//! Task / TaskList (task_list.h): the per-frame task lists of the
//! application. Tasks run in list order; insertion is at the front, the back
//! or after a given task, and the priority only decides which tasks the
//! RemoveUpTo / RemoveFrom sweeps delete.

use super::Cx;
use std::any::Any;

pub trait Task: Any {
    /// Returns false when the task cannot start; it is then dropped.
    fn initialize(&mut self, _cx: &mut Cx) -> bool {
        true
    }
    /// Called right before the task is dropped.
    fn finalize(&mut self, _cx: &mut Cx) {}
    /// True keeps the task alive in `remove_up_to`. Called once per frame by
    /// that sweep, which some tasks use as their per-frame update.
    fn is_protected(&mut self, _cx: &mut Cx) -> bool {
        false
    }
    /// Called every frame while the task is enabled.
    fn run(&mut self, _cx: &mut Cx) {}
    fn enabled(&self) -> bool {
        true
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TaskId(u64);

/// Where `TaskList::add` inserts.
#[derive(Clone, Copy, Debug)]
pub enum Place {
    Front,
    Back,
    After(TaskId),
}

struct Slot {
    id: TaskId,
    priority: u16,
    /// None while the task is running (taken out of the list).
    task: Option<Box<dyn Task>>,
}

#[derive(Default)]
pub struct TaskList {
    slots: Vec<Slot>,
    next_id: u64,
}

/// Which list of the application.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListId {
    Tasks,
    Overlays,
}

impl TaskList {
    fn index(&self, id: TaskId) -> Option<usize> {
        self.slots.iter().position(|s| s.id == id)
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// The task `id` as a `T`.
    pub fn get_mut<T: Task>(&mut self, id: TaskId) -> Option<&mut T> {
        let i = self.index(id)?;
        let task: &mut dyn Any = self.slots[i].task.as_deref_mut()?;
        task.downcast_mut::<T>()
    }
}

fn list(cx: &mut Cx, which: ListId) -> &mut TaskList {
    match which {
        ListId::Tasks => &mut cx.tasks,
        ListId::Overlays => &mut cx.overlays,
    }
}

/// TaskList::Add (0x4039f0).
pub fn add(cx: &mut Cx, which: ListId, mut task: Box<dyn Task>, priority: u16, place: Place) -> Option<TaskId> {
    if !task.initialize(cx) {
        task.finalize(cx);
        return None;
    }
    let l = list(cx, which);
    let id = TaskId(l.next_id);
    l.next_id += 1;
    let slot = Slot { id, priority, task: Some(task) };
    let at = match place {
        Place::Front => 0,
        Place::Back => l.slots.len(),
        Place::After(after) => l.index(after).map_or(l.slots.len(), |i| i + 1),
    };
    l.slots.insert(at, slot);
    Some(id)
}

/// Runs `f` on the task at slot `i`, with the task taken out of the list so
/// that it can use the whole context. Returns the task's slot afterwards
/// (None if it was deleted meanwhile) and `f`'s result.
fn with_task(cx: &mut Cx, which: ListId, i: usize, f: &mut dyn FnMut(&mut dyn Task, &mut Cx) -> bool) -> (Option<usize>, bool) {
    let (id, task) = {
        let l = list(cx, which);
        let s = &mut l.slots[i];
        (s.id, s.task.take())
    };
    let Some(mut task) = task else { return (Some(i), true) };
    let result = f(task.as_mut(), cx);
    let l = list(cx, which);
    match l.index(id) {
        Some(j) => {
            l.slots[j].task = Some(task);
            (Some(j), result)
        }
        None => (None, result),
    }
}

/// TaskList::RunAll (0x4039d0).
pub fn run_all(cx: &mut Cx, which: ListId) {
    let mut i = 0;
    while i < list(cx, which).slots.len() {
        let (at, _) = with_task(cx, which, i, &mut |t, cx| {
            if t.enabled() {
                t.run(cx);
            }
            true
        });
        i = at.map_or(i, |j| j + 1);
    }
}

/// Removes the task at `i` (TaskList::Remove 0x403aa0).
fn remove_at(cx: &mut Cx, which: ListId, i: usize) {
    let slot = list(cx, which).slots.remove(i);
    if let Some(mut task) = slot.task {
        task.finalize(cx);
    }
}

/// TaskList::RemoveUpTo (0x403b10): drops every unprotected task with
/// priority <= `priority`.
pub fn remove_up_to(cx: &mut Cx, which: ListId, priority: u16) {
    let mut i = 0;
    while i < list(cx, which).slots.len() {
        if list(cx, which).slots[i].priority > priority {
            i += 1;
            continue;
        }
        let (at, protected) = with_task(cx, which, i, &mut |t, cx| t.is_protected(cx));
        match at {
            Some(j) if protected => i = j + 1,
            Some(j) => {
                remove_at(cx, which, j);
                i = j;
            }
            None => {}
        }
    }
}

/// TaskList::RemoveFrom (0x403b90): drops every task with priority >=
/// `priority`.
pub fn remove_from(cx: &mut Cx, which: ListId, priority: u16) {
    let mut i = 0;
    while i < list(cx, which).slots.len() {
        if list(cx, which).slots[i].priority >= priority {
            remove_at(cx, which, i);
        } else {
            i += 1;
        }
    }
}

/// TaskList::Delete: drops `id` if it is in the list.
pub fn delete(cx: &mut Cx, which: ListId, id: TaskId) {
    if let Some(i) = list(cx, which).index(id) {
        remove_at(cx, which, i);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    struct Probe(&'static str, Rc<RefCell<Vec<&'static str>>>, bool);
    impl Task for Probe {
        fn run(&mut self, _cx: &mut Cx) {
            self.1.borrow_mut().push(self.0);
        }
        fn is_protected(&mut self, _cx: &mut Cx) -> bool {
            self.2
        }
    }

    #[test]
    fn order_and_sweeps() {
        let mut cx = crate::game::tests::cx();
        let log = Rc::new(RefCell::new(Vec::new()));
        let b = add(&mut cx, ListId::Tasks, Box::new(Probe("b", log.clone(), false)), 2, Place::Back).unwrap();
        add(&mut cx, ListId::Tasks, Box::new(Probe("a", log.clone(), true)), 2, Place::Front);
        add(&mut cx, ListId::Tasks, Box::new(Probe("c", log.clone(), false)), 3, Place::After(b));
        run_all(&mut cx, ListId::Tasks);
        assert_eq!(*log.borrow(), ["a", "b", "c"]);
        // RemoveUpTo(2) keeps the protected "a" and the priority-3 "c".
        remove_up_to(&mut cx, ListId::Tasks, 2);
        log.borrow_mut().clear();
        run_all(&mut cx, ListId::Tasks);
        assert_eq!(*log.borrow(), ["a", "c"]);
        remove_from(&mut cx, ListId::Tasks, 3);
        assert_eq!(cx.tasks.len(), 1);
    }
}
