//! GPU frame time from timestamp queries (when the adapter has them; Vulkan
//! on Android does): a timestamp at the start of a frame's first pass and
//! one at the end of its last, resolved into a ring of readback buffers
//! that are read a frame or two later, never waiting on the GPU.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

/// Frames of timestamps in flight.
const SLOTS: usize = 4;
/// Samples averaged into one shown value.
const WINDOW: u32 = 30;

const FREE: u8 = 0;
const SUBMITTED: u8 = 1;
const MAPPING: u8 = 2;
const READY: u8 = 3;

struct Slot {
    readback: wgpu::Buffer,
    state: Arc<AtomicU8>,
}

pub struct GpuTimer {
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    slots: Vec<Slot>,
    /// The slot this frame writes (None: all busy, this frame is not timed).
    current: Option<usize>,
    next: usize,
    /// Nanoseconds per tick.
    period: f32,
    sum: f64,
    count: u32,
    /// The last window's mean (ms) and the last single frame's.
    pub mean_ms: Option<f32>,
    pub last_ms: Option<f32>,
}

impl GpuTimer {
    /// None when the device was created without timestamp queries.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Self> {
        if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            return None;
        }
        let queries = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("frame timestamps"),
            ty: wgpu::QueryType::Timestamp,
            count: 2 * SLOTS as u32,
        });
        let resolve = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("timestamps resolve"),
            size: (SLOTS * 256) as u64,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let slots = (0..SLOTS)
            .map(|_| Slot {
                readback: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("timestamps readback"),
                    size: 16,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                }),
                state: Arc::new(AtomicU8::new(FREE)),
            })
            .collect();
        Some(GpuTimer {
            queries,
            resolve,
            slots,
            current: None,
            next: 0,
            period: queue.get_timestamp_period(),
            sum: 0.0,
            count: 0,
            mean_ms: None,
            last_ms: None,
        })
    }

    /// Picks this frame's slot (reading the finished ones first).
    pub fn begin_frame(&mut self, device: &wgpu::Device) {
        let _ = device.poll(wgpu::PollType::Poll);
        self.collect();
        let i = self.next;
        self.current = (self.slots[i].state.load(Ordering::Acquire) == FREE).then_some(i);
        if self.current.is_some() {
            self.next = (i + 1) % SLOTS;
        }
    }

    /// The timestamp writes of a pass: `begin` and/or `end` of the frame.
    pub fn writes(&self, begin: bool, end: bool) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        if !begin && !end {
            return None;
        }
        let i = self.current? as u32;
        Some(wgpu::RenderPassTimestampWrites {
            query_set: &self.queries,
            beginning_of_pass_write_index: begin.then_some(2 * i),
            end_of_pass_write_index: end.then_some(2 * i + 1),
        })
    }

    /// Resolves this frame's timestamps (after its last pass).
    pub fn resolve(&self, encoder: &mut wgpu::CommandEncoder) {
        let Some(i) = self.current else { return };
        let offset = (i * 256) as u64;
        encoder.resolve_query_set(&self.queries, 2 * i as u32..2 * i as u32 + 2, &self.resolve, offset);
        encoder.copy_buffer_to_buffer(&self.resolve, offset, &self.slots[i].readback, 0, 16);
    }

    /// After the frame's submit: maps the slot for reading when it is done.
    pub fn after_submit(&mut self) {
        let Some(i) = self.current.take() else { return };
        let slot = &self.slots[i];
        slot.state.store(MAPPING, Ordering::Release);
        let state = slot.state.clone();
        slot.readback.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            state.store(if r.is_ok() { READY } else { SUBMITTED }, Ordering::Release);
        });
    }

    /// Reads every slot whose timestamps arrived.
    pub fn collect(&mut self) {
        for slot in &self.slots {
            match slot.state.load(Ordering::Acquire) {
                READY => {}
                SUBMITTED => {
                    // A failed map: give the slot back.
                    slot.state.store(FREE, Ordering::Release);
                    continue;
                }
                _ => continue,
            }
            let ticks = {
                let Ok(data) = slot.readback.slice(..).get_mapped_range() else { continue };
                let t: [u64; 2] = bytemuck::pod_read_unaligned(&data[..16]);
                t
            };
            slot.readback.unmap();
            slot.state.store(FREE, Ordering::Release);
            if ticks[1] <= ticks[0] {
                continue;
            }
            let ms = (ticks[1] - ticks[0]) as f64 * self.period as f64 / 1e6;
            // Ignore nonsense (a reset counter, a suspended device).
            if !(0.0..1000.0).contains(&ms) {
                continue;
            }
            self.last_ms = Some(ms as f32);
            self.sum += ms;
            self.count += 1;
            if self.count >= WINDOW {
                self.mean_ms = Some((self.sum / self.count as f64) as f32);
                self.sum = 0.0;
                self.count = 0;
            }
        }
    }
}
