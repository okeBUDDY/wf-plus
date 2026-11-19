//! Reads the vector art SVGs. Only what the art uses is supported: fill-only
//! paths with M/L/Q/Z (absolute or relative), and the `data-*` shader
//! attributes. Gradients, filters and masks are browser-preview stand-ins for
//! those attributes and are ignored.

use crate::geom::{Curve, Vec2};

#[derive(Clone, Debug)]
pub enum RampGeom {
    /// From `p0` to `p1`.
    Linear(Vec2, Vec2),
    Radial(Vec2, f32),
}

#[derive(Clone, Debug)]
pub struct Ramp {
    pub geom: RampGeom,
    /// (offset, straight RGBA 0..1)
    pub stops: Vec<(f32, [f32; 4])>,
}

#[derive(Clone, Debug)]
pub enum Paint {
    Solid([f32; 4]),
    Ramp(Ramp),
}

#[derive(Clone, Debug)]
pub struct Path {
    pub curves: Vec<Curve>,
    pub paint: Paint,
    pub heat: Option<f32>,
}

#[derive(Clone, Debug, Default)]
pub struct Noise {
    pub frequency: f32,
    pub octaves: f32,
    pub seed: f32,
    pub amplitude: f32,
}

#[derive(Clone, Debug)]
pub struct Art {
    pub width: f32,
    pub height: f32,
    pub origin: Vec2,
    pub additive: bool,
    pub layers_max: bool,
    pub noise: Option<Noise>,
    /// sys0_008's combined ramps: (stripe ramp).
    pub combine_stripe: Option<Ramp>,
    pub paths: Vec<Path>,
}

pub type Error = String;

fn floats(s: &str) -> Vec<f32> {
    s.split(|c: char| c.is_whitespace() || c == ',')
        .filter(|t| !t.is_empty())
        .filter_map(|t| t.parse().ok())
        .collect()
}

fn hex_byte(s: &str) -> Result<f32, Error> {
    // Some cooked stops carry an alpha above 0xff (sys0_008); clamp it.
    let v = u32::from_str_radix(s, 16).map_err(|_| format!("bad colour '{s}'"))?;
    Ok(v.min(255) as f32 / 255.0)
}

/// `#rgb`, `#rrggbb` or `#rrggbb<alpha hex>`.
pub fn parse_color(s: &str) -> Result<[f32; 4], Error> {
    let h = s.strip_prefix('#').ok_or_else(|| format!("bad colour '{s}'"))?;
    if h.len() == 3 {
        let c: Vec<f32> = h.chars().map(|c| u32::from_str_radix(&c.to_string(), 16).unwrap_or(0) as f32 / 15.0).collect();
        return Ok([c[0], c[1], c[2], 1.0]);
    }
    if h.len() < 6 {
        return Err(format!("bad colour '{s}'"));
    }
    let a = if h.len() > 6 { hex_byte(&h[6..])? } else { 1.0 };
    Ok([hex_byte(&h[0..2])?, hex_byte(&h[2..4])?, hex_byte(&h[4..6])?, a])
}

fn parse_stops(s: &str) -> Result<Vec<(f32, [f32; 4])>, Error> {
    s.split_whitespace()
        .map(|item| {
            let (o, c) = item.split_once(':').ok_or_else(|| format!("bad stop '{item}'"))?;
            Ok((o.parse().map_err(|_| format!("bad stop '{item}'"))?, parse_color(c)?))
        })
        .collect()
}

/// `linear x a b`, `linear y a b`, `linear x1 y1 x2 y2`, `radial cx cy r`.
fn parse_ramp_geom(s: &str, art_w: f32, art_h: f32) -> Result<RampGeom, Error> {
    let mut words = s.split_whitespace();
    let kind = words.next().unwrap_or("");
    let rest: Vec<&str> = words.collect();
    let _ = (art_w, art_h);
    match kind {
        "linear" => match rest.first().copied() {
            Some("x") | Some("y") => {
                let v = floats(&rest[1..].join(" "));
                if v.len() != 2 {
                    return Err(format!("bad ramp '{s}'"));
                }
                Ok(if rest[0] == "x" {
                    RampGeom::Linear(Vec2::new(v[0], 0.0), Vec2::new(v[1], 0.0))
                } else {
                    RampGeom::Linear(Vec2::new(0.0, v[0]), Vec2::new(0.0, v[1]))
                })
            }
            _ => {
                let v = floats(&rest.join(" "));
                if v.len() != 4 {
                    return Err(format!("bad ramp '{s}'"));
                }
                Ok(RampGeom::Linear(Vec2::new(v[0], v[1]), Vec2::new(v[2], v[3])))
            }
        },
        "radial" => {
            let v = floats(&rest.join(" "));
            if v.len() != 3 {
                return Err(format!("bad ramp '{s}'"));
            }
            Ok(RampGeom::Radial(Vec2::new(v[0], v[1]), v[2]))
        }
        _ => Err(format!("unknown ramp '{s}'")),
    }
}

/// Parses path data into quadratic curves (lines become curves whose
/// control point is the midpoint). Unclosed subpaths are closed, as a fill
/// would close them.
pub fn parse_path(d: &str) -> Result<Vec<Curve>, Error> {
    let mut curves = Vec::new();
    let bytes = d.as_bytes();
    let mut i = 0;
    let mut cmd = b'M';
    let mut cur = Vec2::new(0.0, 0.0);
    let mut start = cur;
    let mut open = false;

    let close = |curves: &mut Vec<Curve>, cur: &mut Vec2, start: Vec2, open: &mut bool| {
        if *open && (cur.x != start.x || cur.y != start.y) {
            curves.push(Curve::line(*cur, start));
        }
        *cur = start;
        *open = false;
    };

    let number = |i: &mut usize| -> Result<f32, Error> {
        while *i < bytes.len() && (bytes[*i].is_ascii_whitespace() || bytes[*i] == b',') {
            *i += 1;
        }
        let s = *i;
        if *i < bytes.len() && (bytes[*i] == b'-' || bytes[*i] == b'+') {
            *i += 1;
        }
        let mut dot = false;
        while *i < bytes.len() {
            let c = bytes[*i];
            if c.is_ascii_digit() {
                *i += 1;
            } else if c == b'.' && !dot {
                dot = true;
                *i += 1;
            } else if (c == b'e' || c == b'E') && *i > s {
                *i += 1;
                if *i < bytes.len() && (bytes[*i] == b'-' || bytes[*i] == b'+') {
                    *i += 1;
                }
            } else {
                break;
            }
        }
        d[s..*i].parse::<f32>().map_err(|_| format!("bad number at {s} in path"))
    };

    loop {
        while i < bytes.len() && (bytes[i].is_ascii_whitespace() || bytes[i] == b',') {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let c = bytes[i];
        if c.is_ascii_alphabetic() {
            cmd = c;
            i += 1;
            if cmd == b'Z' || cmd == b'z' {
                close(&mut curves, &mut cur, start, &mut open);
                continue;
            }
        }
        let rel = cmd.is_ascii_lowercase();
        let base = if rel { cur } else { Vec2::new(0.0, 0.0) };
        match cmd.to_ascii_uppercase() {
            b'M' => {
                close(&mut curves, &mut cur, start, &mut open);
                let p = Vec2::new(number(&mut i)? + base.x, number(&mut i)? + base.y);
                cur = p;
                start = p;
                open = true;
                // Further pairs are line-tos.
                cmd = if rel { b'l' } else { b'L' };
            }
            b'L' => {
                let p = Vec2::new(number(&mut i)? + base.x, number(&mut i)? + base.y);
                curves.push(Curve::line(cur, p));
                cur = p;
            }
            b'H' => {
                let x = number(&mut i)? + base.x;
                let p = Vec2::new(x, cur.y);
                curves.push(Curve::line(cur, p));
                cur = p;
            }
            b'V' => {
                let y = number(&mut i)? + base.y;
                let p = Vec2::new(cur.x, y);
                curves.push(Curve::line(cur, p));
                cur = p;
            }
            b'Q' => {
                let c1 = Vec2::new(number(&mut i)? + base.x, number(&mut i)? + base.y);
                let p = Vec2::new(number(&mut i)? + base.x, number(&mut i)? + base.y);
                curves.push(Curve { p1: cur, p2: c1, p3: p });
                cur = p;
            }
            other => return Err(format!("unsupported path command '{}'", other as char)),
        }
    }
    close(&mut curves, &mut cur, start, &mut open);
    // Zero-length segments add nothing.
    curves.retain(|c| !(c.p1 == c.p3 && c.p1 == c.p2));
    Ok(curves)
}

fn inside_preview_only(node: roxmltree::Node) -> bool {
    node.ancestors().any(|a| matches!(a.tag_name().name(), "defs" | "mask" | "clipPath" | "metadata"))
}

pub fn parse(text: &str) -> Result<Art, Error> {
    let doc = roxmltree::Document::parse(text).map_err(|e| e.to_string())?;
    let root = doc.root_element();
    let num = |n: roxmltree::Node, a: &str| n.attribute(a).and_then(|v| v.trim_end_matches("px").parse::<f32>().ok());
    let width = num(root, "width").unwrap_or(0.0);
    let height = num(root, "height").unwrap_or(0.0);
    let origin = match root.attribute("data-origin") {
        Some(s) => {
            let v = floats(s);
            Vec2::new(v[0], v[1])
        }
        // The extras have no pivot: they are placed by their top-left corner.
        None => Vec2::new(0.0, 0.0),
    };
    let stops = root.attribute("data-stops").map(parse_stops).transpose()?;
    let root_ramp = match (root.attribute("data-ramp"), &stops) {
        (Some(r), Some(stops)) if r != "heat" => Some(Ramp { geom: parse_ramp_geom(r, width, height)?, stops: stops.clone() }),
        _ => None,
    };
    let combine = root.attribute("data-combine").is_some();
    let combine_stripe = if combine {
        let g = root.attribute("data-ramp2").ok_or("data-combine without data-ramp2")?;
        let s = root.attribute("data-stops2").ok_or("data-combine without data-stops2")?;
        Some(Ramp { geom: parse_ramp_geom(g, width, height)?, stops: parse_stops(s)? })
    } else {
        None
    };
    let noise = root.attribute("data-noise").map(|s| {
        let mut w = s.split_whitespace();
        let _kind = w.next();
        let v = floats(&w.collect::<Vec<_>>().join(" "));
        Noise {
            frequency: *v.first().unwrap_or(&0.05),
            octaves: *v.get(1).unwrap_or(&1.0),
            seed: *v.get(2).unwrap_or(&0.0),
            amplitude: *v.get(3).unwrap_or(&1.0),
        }
    });

    let mut paths = Vec::new();
    for node in root.descendants().filter(|n| n.has_tag_name("path")) {
        if inside_preview_only(node) || node.attribute("mask").is_some() {
            continue;
        }
        if let Some(rule) = node.attribute("fill-rule") {
            if rule != "nonzero" {
                return Err(format!("unsupported fill-rule '{rule}'"));
            }
        }
        let fill = node.attribute("fill").unwrap_or("#000000");
        if fill == "none" {
            continue;
        }
        let opacity = num(node, "fill-opacity").unwrap_or(1.0) * num(node, "opacity").unwrap_or(1.0);
        let paint = if let Some(r) = node.attribute("data-ramp") {
            let stops = match node.attribute("data-stops") {
                Some(own) => parse_stops(own)?,
                None => stops.clone().ok_or("path data-ramp without data-stops")?,
            };
            Paint::Ramp(Ramp { geom: parse_ramp_geom(r, width, height)?, stops })
        } else if fill.starts_with("url(") {
            Paint::Ramp(root_ramp.clone().ok_or("gradient fill without a data-ramp")?)
        } else {
            let mut c = parse_color(fill)?;
            c[3] *= opacity;
            Paint::Solid(c)
        };
        let curves = parse_path(node.attribute("d").unwrap_or(""))?;
        if curves.is_empty() {
            continue;
        }
        let heat = num(node, "data-heat");
        paths.push(Path { curves, paint, heat });
    }
    // The combined-ramp art is one shape whose paint the shader computes.
    if combine {
        paths.truncate(1);
    }
    Ok(Art {
        width,
        height,
        origin,
        additive: root.attribute("data-blend") == Some("additive"),
        layers_max: root.attribute("data-blend-layers") == Some("max"),
        noise,
        combine_stripe,
        paths,
    })
}
