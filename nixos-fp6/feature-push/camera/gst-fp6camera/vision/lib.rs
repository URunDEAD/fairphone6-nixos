// SPDX-License-Identifier: LGPL-2.1-or-later
//
// fp6-vision: panorama (sweep tracking + cylindrical stitching) for the
// FP6 camera (camera phase 2). No dependencies; built with plain rustc as a
// static library with a small C ABI (fp6-vision.h) that fp6libcamerasrc
// (C, GStreamer) calls. All images are 8-bit RGB, tightly packed rows
// unless a stride is given.

#![allow(clippy::too_many_arguments)]

use std::os::raw::{c_double, c_int, c_void};
use std::slice;

// ---------------------------------------------------------------------------
// threads: the caller runs us in a low-priority thread; new threads inherit
// its nice value

pub const THREADS: usize = 4;

/// run f(chunk_index, chunk) over `rows` rows of `out` (row_len elements per
/// row) split into THREADS bands
fn par_rows<T: Send, F: Fn(usize, &mut [T]) + Sync>(out: &mut [T], row_len: usize, f: F) {
    let rows = out.len() / row_len.max(1);
    let band = (rows + THREADS - 1) / THREADS;
    if band == 0 {
        return;
    }
    std::thread::scope(|sc| {
        for (i, chunk) in out.chunks_mut(band * row_len).enumerate() {
            let f = &f;
            sc.spawn(move || f(i * band, chunk));
        }
    });
}

// ---------------------------------------------------------------------------
// images

#[derive(Clone)]
pub struct Img {
    pub w: usize,
    pub h: usize,
    pub d: Vec<u8>, // w * h * 3
}

impl Img {
    pub fn new(w: usize, h: usize) -> Img {
        Img { w, h, d: vec![0; w * h * 3] }
    }
    #[inline]
    fn px(&self, x: usize, y: usize) -> &[u8] {
        let o = (y * self.w + x) * 3;
        &self.d[o..o + 3]
    }
    /// bilinear sample, None outside
    #[inline]
    fn sample(&self, x: f64, y: f64) -> Option<[f32; 3]> {
        if x < 0.0 || y < 0.0 || x > (self.w - 1) as f64 || y > (self.h - 1) as f64 {
            return None;
        }
        let x0 = x as usize;
        let y0 = y as usize;
        let x1 = (x0 + 1).min(self.w - 1);
        let y1 = (y0 + 1).min(self.h - 1);
        let ax = (x - x0 as f64) as f32;
        let ay = (y - y0 as f64) as f32;
        let (a, b, c, d) = (self.px(x0, y0), self.px(x1, y0), self.px(x0, y1), self.px(x1, y1));
        let mut o = [0f32; 3];
        for k in 0..3 {
            let t = a[k] as f32 * (1.0 - ax) + b[k] as f32 * ax;
            let u = c[k] as f32 * (1.0 - ax) + d[k] as f32 * ax;
            o[k] = t * (1.0 - ay) + u * ay;
        }
        Some(o)
    }
}

pub struct Gray {
    pub w: usize,
    pub h: usize,
    pub d: Vec<u8>,
    /// optional validity mask (true = usable); empty = all valid
    pub m: Vec<bool>,
}

fn luma(p: &[u8]) -> u8 {
    ((p[0] as u32 * 77 + p[1] as u32 * 150 + p[2] as u32 * 29) >> 8) as u8
}

impl Gray {
    fn from_img(img: &Img) -> Gray {
        let d = img.d.chunks_exact(3).map(luma).collect();
        Gray { w: img.w, h: img.h, d, m: vec![] }
    }
    fn valid(&self, i: usize) -> bool {
        self.m.is_empty() || self.m[i]
    }
    /// shrink the valid area by r pixels (gradients at the mask edge are fake)
    fn erode(&mut self, r: usize) {
        if self.m.is_empty() {
            return;
        }
        for _ in 0..r {
            let old = self.m.clone();
            for y in 0..self.h {
                for x in 0..self.w {
                    let i = y * self.w + x;
                    if old[i]
                        && (x == 0 || y == 0 || x + 1 == self.w || y + 1 == self.h
                            || !old[i - 1] || !old[i + 1] || !old[i - self.w] || !old[i + self.w])
                    {
                        self.m[i] = false;
                    }
                }
            }
        }
    }
    fn half(&self) -> Gray {
        let (w, h) = (self.w / 2, self.h / 2);
        let mut d = vec![0u8; w * h];
        for y in 0..h {
            for x in 0..w {
                let a = &self.d[(2 * y) * self.w + 2 * x..];
                let b = &self.d[(2 * y + 1) * self.w + 2 * x..];
                d[y * w + x] = ((a[0] as u32 + a[1] as u32 + b[0] as u32 + b[1] as u32 + 2) >> 2) as u8;
            }
        }
        let mut m = vec![];
        if !self.m.is_empty() {
            m = vec![false; w * h];
            for y in 0..h {
                for x in 0..w {
                    let i = (2 * y) * self.w + 2 * x;
                    m[y * w + x] = self.m[i] && self.m[i + 1] && self.m[i + self.w] && self.m[i + self.w + 1];
                }
            }
        }
        Gray { w, h, d, m }
    }
    /// gradient magnitude (|gx| + |gy|), saturated
    fn gradient(&self) -> Gray {
        let mut d = vec![0u8; self.w * self.h];
        for y in 1..self.h.saturating_sub(1) {
            for x in 1..self.w - 1 {
                let i = y * self.w + x;
                let gx = self.d[i + 1] as i32 - self.d[i - 1] as i32;
                let gy = self.d[i + self.w] as i32 - self.d[i - self.w] as i32;
                d[i] = (gx.abs() + gy.abs()).min(255) as u8;
            }
        }
        let mut g = Gray { w: self.w, h: self.h, d, m: self.m.clone() };
        g.erode(2);
        g
    }
}

/// EXIF orientation (1..8) applied to an image: the displayed image
pub fn orient(img: &Img, code: u16) -> Img {
    let (w, h) = (img.w, img.h);
    let swap = (5..=8).contains(&code);
    let (ow, oh) = if swap { (h, w) } else { (w, h) };
    let mut o = Img::new(ow, oh);
    for y in 0..oh {
        for x in 0..ow {
            let (sx, sy) = match code {
                2 => (w - 1 - x, y),
                3 => (w - 1 - x, h - 1 - y),
                4 => (x, h - 1 - y),
                5 => (y, x),
                6 => (y, h - 1 - x),
                7 => (w - 1 - y, h - 1 - x),
                8 => (w - 1 - y, x),
                _ => (x, y),
            };
            let s = (sy * w + sx) * 3;
            let d = (y * ow + x) * 3;
            o.d[d..d + 3].copy_from_slice(&img.d[s..s + 3]);
        }
    }
    o
}

fn orient_gray(g: &Gray, code: u16) -> Gray {
    let img = Img { w: g.w, h: g.h, d: g.d.iter().flat_map(|&v| [v, v, v]).collect() };
    let o = orient(&img, code);
    Gray { w: o.w, h: o.h, d: o.d.chunks_exact(3).map(|p| p[0]).collect(), m: vec![] }
}

/// mean absolute difference of b shifted by (dx, dy) against a over the
/// overlap (sampling every `step` pixels); None if the overlap is too small
fn mad(a: &Gray, b: &Gray, dx: i64, dy: i64, step: usize, min_frac: f64) -> Option<f64> {
    // a(x, y) ~ b(x - dx, y - dy)  (b placed at (dx, dy) in a's frame)
    let x0 = dx.max(0);
    let y0 = dy.max(0);
    let x1 = (a.w as i64).min(b.w as i64 + dx);
    let y1 = (a.h as i64).min(b.h as i64 + dy);
    if x1 - x0 < (a.w as f64 * min_frac) as i64 || y1 - y0 < (a.h as f64 * 0.3) as i64 {
        return None;
    }
    let mut s: u64 = 0;
    let mut n: u64 = 0;
    let mut y = y0;
    while y < y1 {
        let ia = y as usize * a.w;
        let ib = (y - dy) as usize * b.w;
        let (ra, rb) = (&a.d[ia..], &b.d[ib..]);
        let masked = !a.m.is_empty() || !b.m.is_empty();
        let mut x = x0;
        while x < x1 {
            if !masked || (a.valid(ia + x as usize) && b.valid(ib + (x - dx) as usize)) {
                s += (ra[x as usize] as i32 - rb[(x - dx) as usize] as i32).unsigned_abs() as u64;
                n += 1;
            }
            x += step as i64;
        }
        y += step as i64;
    }
    // the overlap must also be mostly valid
    let area = ((x1 - x0) as u64 * (y1 - y0) as u64) / (step * step) as u64;
    if n == 0 || n * 3 < area {
        None
    } else {
        Some(s as f64 / n as f64)
    }
}

/// best placement of b in a's frame within the given search window
fn search(a: &Gray, b: &Gray, cx: i64, cy: i64, rx: i64, ry: i64, step: usize, min_frac: f64) -> (i64, i64, f64) {
    let rows: Vec<i64> = (cy - ry..=cy + ry).collect();
    let per = (rows.len() + THREADS - 1) / THREADS;
    let results: Vec<(i64, i64, f64)> = std::thread::scope(|sc| {
        let hs: Vec<_> = rows
            .chunks(per.max(1))
            .map(|ch| {
                sc.spawn(move || {
                    let mut best = (cx, cy, f64::MAX);
                    for &dy in ch {
                        for dx in cx - rx..=cx + rx {
                            if let Some(e) = mad(a, b, dx, dy, step, min_frac) {
                                // tie break towards the centre of the window
                                let e = e + 1e-6 * ((dx - cx).abs() + (dy - cy).abs()) as f64;
                                if e < best.2 {
                                    best = (dx, dy, e);
                                }
                            }
                        }
                    }
                    best
                })
            })
            .collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    results.into_iter().fold((cx, cy, f64::MAX), |b, r| if r.2 < b.2 { r } else { b })
}

// ---------------------------------------------------------------------------
// panorama

pub struct Pano {
    exif: u16,
    focal: f64,        // full resolution, pixels
    scale: f64,        // full / small (display)
    prev: Option<Gray>, // previous small display frame
    acc: (f64, f64),   // since the last keyframe (small px)
    total: (f64, f64), // since the first keyframe (small px)
    dir: i32,          // +1 scene moves left (sweep right), -1, 0 unknown
    keys: Vec<(f64, f64)>, // keyframe positions, full-res display px (scene coords)
    max_keys: usize,
    small_w: usize,
    small_h: usize,
}

impl Pano {
    pub fn new(exif: u16, focal: f64, scale: f64, max_keys: usize) -> Pano {
        Pano {
            exif,
            focal,
            scale,
            prev: None,
            acc: (0.0, 0.0),
            total: (0.0, 0.0),
            dir: 0,
            keys: Vec::new(),
            max_keys,
            small_w: 0,
            small_h: 0,
        }
    }

    /// One small luma frame (stream orientation). Returns 1 when this frame
    /// should be kept as a keyframe, 2 when the sweep is complete.
    pub fn feed(&mut self, g: &Gray) -> i32 {
        let cur = orient_gray(g, self.exif);
        self.small_w = cur.w;
        self.small_h = cur.h;
        let prev = match self.prev.take() {
            None => {
                self.prev = Some(cur);
                self.keys.push((0.0, 0.0));
                return 1;
            }
            Some(p) => p,
        };
        // frame-to-frame motion: coarse at half size, refined
        let (ph, ch) = (prev.half(), cur.half());
        let (cx, cy, _) = search(&ph, &ch, 0, 0, 8, 4, 2, 0.5);
        let (dx, dy, _) = search(&prev, &cur, cx * 2, cy * 2, 1, 1, 2, 0.5);
        // cur sits at (dx, dy) in prev's frame: the camera turned that way
        self.acc.0 += dx as f64;
        self.acc.1 += dy as f64;
        self.total.0 += dx as f64;
        self.total.1 += dy as f64;
        if self.dir == 0 && self.total.0.abs() > 0.06 * cur.w as f64 {
            self.dir = if self.total.0 > 0.0 { 1 } else { -1 };
        }
        self.prev = Some(cur);
        if self.dir != 0 && self.acc.0 * self.dir as f64 >= 0.33 * self.small_w as f64 {
            self.acc = (0.0, 0.0);
            self.keys.push((self.total.0 * self.scale, self.total.1 * self.scale));
            return if self.keys.len() >= self.max_keys { 2 } else { 1 };
        }
        0
    }

    /// (progress of the current step 0..1, vertical drift in frame heights
    /// since the start, direction)
    pub fn status(&self) -> (f64, f64, i32) {
        if self.small_w == 0 {
            return (0.0, 0.0, 0);
        }
        let step = (self.acc.0 * if self.dir == 0 { 1.0 } else { self.dir as f64 }).abs()
            / (0.33 * self.small_w as f64);
        (step.min(1.0), self.total.1 / self.small_h as f64, self.dir)
    }
}

fn cylindrical(img: &Img, f: f64) -> (Img, Vec<bool>) {
    let (w, h) = (img.w, img.h);
    let (cx, cy) = (w as f64 / 2.0, h as f64 / 2.0);
    let cols: Vec<(f64, f64)> = (0..w)
        .map(|u| {
            let th = (u as f64 - cx) / f;
            (f * th.tan() + cx, th.cos())
        })
        .collect();
    let mut o = Img::new(w, h);
    par_rows(&mut o.d, w * 3, |v0, band| {
        for (r, row) in band.chunks_mut(w * 3).enumerate() {
            let v = v0 + r;
            for u in 0..w {
                let (x, c) = cols[u];
                let y = (v as f64 - cy) / c + cy;
                if let Some(p) = img.sample(x, y) {
                    // 0 is reserved for "outside" (the mask below)
                    row[u * 3] = (p[0] + 0.5).max(1.0) as u8;
                    row[u * 3 + 1] = (p[1] + 0.5) as u8;
                    row[u * 3 + 2] = (p[2] + 0.5) as u8;
                }
            }
        }
    });
    let mut valid = vec![false; w * h];
    for v in 0..h {
        for u in 0..w {
            let (x, c) = cols[u];
            let y = (v as f64 - cy) / c + cy;
            valid[v * w + u] = x >= 0.0 && y >= 0.0 && x <= (w - 1) as f64 && y <= (h - 1) as f64;
        }
    }
    (o, valid)
}

/// Stitch display-oriented keyframes; keys = rough positions (full-res px).
pub fn stitch(frames: &[Img], keys: &[(f64, f64)], focal: f64, progress: &mut dyn FnMut(f64)) -> Option<Img> {
    let n = frames.len();
    if n < 2 {
        return None;
    }
    let (w, h) = (frames[0].w, frames[0].h);
    let f = focal.max(w.max(h) as f64 * 0.3);
    let dbg = std::env::var_os("FP6_VISION_DEBUG").is_some();
    let t0 = std::time::Instant::now();
    // 1. cylindrical projection
    let mut warped = Vec::with_capacity(n);
    for (i, fr) in frames.iter().enumerate() {
        warped.push(cylindrical(fr, f));
        progress(0.3 * (i + 1) as f64 / n as f64);
    }
    if dbg { eprintln!("warp {:?}", t0.elapsed()); }
    // 2. pairwise alignment on gradients (coarse to fine)
    let mut pos = vec![(0i64, 0i64); n];
    for i in 1..n {
        let mut a = Gray::from_img(&warped[i - 1].0);
        let mut b = Gray::from_img(&warped[i].0);
        a.m = warped[i - 1].1.clone();
        b.m = warped[i].1.clone();
        // gradient pyramids; exhaustive search on the level about 256 px
        // wide (cheap, still detailed), then +-2 px per finer level
        let mut pa = vec![a.gradient()];
        let mut pb = vec![b.gradient()];
        while pa.len() < 4 && pa.last().unwrap().w / 2 >= 200 {
            let (na, nb) = (pa.last().unwrap().half(), pb.last().unwrap().half());
            pa.push(na);
            pb.push(nb);
        }
        // the tracker's estimate, converted to cylindrical x
        let ex = keys.get(i).map(|k| k.0).unwrap_or(0.0) - keys.get(i - 1).map(|k| k.0).unwrap_or(0.0);
        let ey = keys.get(i).map(|k| k.1).unwrap_or(0.0) - keys.get(i - 1).map(|k| k.1).unwrap_or(0.0);
        let ex = f * (ex / f).atan();
        let top = pa.len() - 1;
        let sc = (1usize << top) as f64;
        let rx = (0.15 * w as f64 / sc) as i64;
        let ry = (0.06 * h as f64 / sc) as i64;
        let (mut x1, mut y1, _) = search(&pa[top], &pb[top], (ex / sc).round() as i64, (ey / sc).round() as i64,
                                         rx, ry, 1, 0.15);
        for l in (0..top).rev() {
            let r = search(&pa[l], &pb[l], x1 * 2, y1 * 2, 2, 2, if l == 0 { 2 } else { 1 }, 0.15);
            x1 = r.0;
            y1 = r.1;
        }
        pos[i] = (pos[i - 1].0 + x1, pos[i - 1].1 + y1);
        progress(0.3 + 0.4 * i as f64 / (n - 1) as f64);
    }
    if dbg { eprintln!("align {:?}", t0.elapsed()); }
    // 3. order along the sweep, exposure gains from the overlaps
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by_key(|&i| pos[i].0);
    let mut gain = vec![1f32; n];
    for k in 1..n {
        let (i, j) = (order[k - 1], order[k]);
        let (dx, dy) = (pos[j].0 - pos[i].0, pos[j].1 - pos[i].1);
        let (mut si, mut sj, mut cnt) = (0f64, 0f64, 0u64);
        let mut y = 0i64.max(dy);
        while y < (h as i64).min(h as i64 + dy) {
            let mut x = dx;
            while x < w as i64 {
                let (xi, yi) = (x as usize, y as usize);
                let (xj, yj) = ((x - dx) as usize, (y - dy) as usize);
                if warped[i].1[yi * w + xi] && warped[j].1[yj * w + xj] {
                    si += luma(warped[i].0.px(xi, yi)) as f64;
                    sj += luma(warped[j].0.px(xj, yj)) as f64;
                    cnt += 1;
                }
                x += 4;
            }
            y += 4;
        }
        let r = if cnt > 100 && sj > 0.0 { (si / sj) as f32 } else { 1.0 };
        gain[j] = gain[i] * r.clamp(0.7, 1.4);
    }
    let gm = (gain.iter().map(|g| (*g as f64).ln()).sum::<f64>() / n as f64).exp() as f32;
    for g in gain.iter_mut() {
        *g /= gm;
    }
    // 4. canvas: vertical crop to rows every frame covers
    let vmargin = (h as f64 / 2.0) * (1.0 - f / ((w as f64 / 2.0).powi(2) + f * f).sqrt());
    let x0 = pos.iter().map(|p| p.0).min().unwrap();
    let x1 = pos.iter().map(|p| p.0).max().unwrap() + w as i64;
    let top = pos.iter().map(|p| p.1).max().unwrap() + vmargin.ceil() as i64;
    let bot = pos.iter().map(|p| p.1).min().unwrap() + h as i64 - vmargin.ceil() as i64;
    if bot - top < (h / 3) as i64 || x1 - x0 <= w as i64 {
        return None;
    }
    let (cw, ch) = ((x1 - x0) as usize, (bot - top) as usize);
    // seams between neighbours: middle of their overlap, feather band
    let mut seams = vec![];
    for k in 1..n {
        let (i, j) = (order[k - 1], order[k]);
        let ov_l = pos[j].0;
        let ov_r = pos[i].0 + w as i64;
        seams.push((((ov_l + ov_r) as f64 / 2.0), ((ov_r - ov_l) as f64 * 0.5).clamp(8.0, w as f64 * 0.25)));
    }
    let mut out = Img::new(cw, ch);
    let (warped_r, order_r, seams_r, gain_r, pos_r) = (&warped, &order, &seams, &gain, &pos);
    par_rows(&mut out.d, cw * 3, |r0, band| {
        for (r, orow) in band.chunks_mut(cw * 3).enumerate() {
            let cyv = top + (r0 + r) as i64;
            for col in 0..cw {
                let cx = x0 + col as i64;
                let mut acc = [0f32; 3];
                let mut ws = 0f32;
                for k in 0..n {
                    let i = order_r[k];
                    let (lx, ly) = (cx - pos_r[i].0, cyv - pos_r[i].1);
                    if lx < 0 || ly < 0 || lx >= w as i64 || ly >= h as i64 {
                        continue;
                    }
                    let (lx, ly) = (lx as usize, ly as usize);
                    if !warped_r[i].1[ly * w + lx] {
                        continue;
                    }
                    let mut wt = 1f32;
                    if k > 0 {
                        let (s, b) = seams_r[k - 1];
                        wt *= (((cx as f64 - (s - b / 2.0)) / b).clamp(0.0, 1.0)) as f32;
                    }
                    if k + 1 < n {
                        let (s, b) = seams_r[k];
                        wt *= ((((s + b / 2.0) - cx as f64) / b).clamp(0.0, 1.0)) as f32;
                    }
                    if wt <= 0.0 {
                        continue;
                    }
                    let p = warped_r[i].0.px(lx, ly);
                    for c in 0..3 {
                        acc[c] += wt * p[c] as f32 * gain_r[i];
                    }
                    ws += wt;
                }
                if ws > 0.0 {
                    for c in 0..3 {
                        orow[col * 3 + c] = (acc[c] / ws + 0.5).clamp(0.0, 255.0) as u8;
                    }
                }
            }
        }
    });
    progress(1.0);
    if dbg { eprintln!("composite {:?}", t0.elapsed()); }
    if dbg {
        eprintln!("pano positions {:?} gains {:?} canvas {}x{}", pos, gain, cw, ch);
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// C ABI (see fp6-vision.h)

#[repr(C)]
pub struct Fp6vImage {
    pub w: c_int,
    pub h: c_int,
    pub stride: c_int, // bytes per row
    pub ps: c_int,     // bytes per pixel (3 or 4)
    pub ro: c_int,
    pub go: c_int,
    pub bo: c_int,
    pub data: *mut u8,
}

pub type ProgressFn = Option<extern "C" fn(*mut c_void, c_double)>;

unsafe fn img_from_c(i: &Fp6vImage) -> Img {
    let (w, h) = (i.w as usize, i.h as usize);
    let src = slice::from_raw_parts(i.data, i.stride as usize * h);
    let mut o = Img::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let p = &src[y * i.stride as usize + x * i.ps as usize..];
            let d = (y * w + x) * 3;
            o.d[d] = p[i.ro as usize];
            o.d[d + 1] = p[i.go as usize];
            o.d[d + 2] = p[i.bo as usize];
        }
    }
    o
}

/// hand an image to C: RGB, rows padded to 4 bytes (GStreamer RGB layout);
/// freed with fp6v_free
fn img_to_c(img: Img, out: &mut Fp6vImage) {
    let stride = (img.w * 3 + 3) & !3;
    let mut v = vec![0u8; stride * img.h].into_boxed_slice();
    for y in 0..img.h {
        v[y * stride..y * stride + img.w * 3].copy_from_slice(&img.d[y * img.w * 3..(y + 1) * img.w * 3]);
    }
    out.w = img.w as c_int;
    out.h = img.h as c_int;
    out.stride = stride as c_int;
    out.ps = 3;
    out.ro = 0;
    out.go = 1;
    out.bo = 2;
    out.data = Box::into_raw(v) as *mut u8;
}

#[no_mangle]
pub unsafe extern "C" fn fp6v_free(img: *mut Fp6vImage) {
    if img.is_null() || (*img).data.is_null() {
        return;
    }
    let len = (*img).stride as usize * (*img).h as usize;
    drop(Box::from_raw(slice::from_raw_parts_mut((*img).data, len) as *mut [u8]));
    (*img).data = std::ptr::null_mut();
}

#[no_mangle]
pub extern "C" fn fp6v_pano_new(exif: u16, focal_px: c_double, scale: c_double, max_keys: c_int) -> *mut Pano {
    Box::into_raw(Box::new(Pano::new(exif, focal_px, scale, max_keys.max(2) as usize)))
}

#[no_mangle]
pub unsafe extern "C" fn fp6v_pano_free(p: *mut Pano) {
    if !p.is_null() {
        drop(Box::from_raw(p));
    }
}

/// luma: w x h bytes, stride `stride`. 0 = nothing, 1 = keep this frame,
/// 2 = keep it and the sweep is complete
#[no_mangle]
pub unsafe extern "C" fn fp6v_pano_feed(p: *mut Pano, luma: *const u8, w: c_int, h: c_int, stride: c_int) -> c_int {
    if p.is_null() || luma.is_null() || w < 16 || h < 16 {
        return 0;
    }
    let src = slice::from_raw_parts(luma, stride as usize * h as usize);
    let mut d = Vec::with_capacity((w * h) as usize);
    for y in 0..h as usize {
        d.extend_from_slice(&src[y * stride as usize..y * stride as usize + w as usize]);
    }
    (*p).feed(&Gray { w: w as usize, h: h as usize, d, m: vec![] })
}

#[no_mangle]
pub unsafe extern "C" fn fp6v_pano_status(p: *const Pano, step: *mut c_double, drift: *mut c_double, dir: *mut c_int) {
    let (s, d, r) = if p.is_null() { (0.0, 0.0, 0) } else { (*p).status() };
    if !step.is_null() { *step = s; }
    if !drift.is_null() { *drift = d; }
    if !dir.is_null() { *dir = r; }
}

#[no_mangle]
pub unsafe extern "C" fn fp6v_pano_keys(p: *const Pano) -> c_int {
    if p.is_null() { 0 } else { (*p).keys.len() as c_int }
}

/// frames: n keyframes in stream orientation, in the order fed. 0 = ok.
#[no_mangle]
pub unsafe extern "C" fn fp6v_pano_stitch(p: *const Pano, frames: *const Fp6vImage, n: c_int, out: *mut Fp6vImage,
                                          progress: ProgressFn, user: *mut c_void) -> c_int {
    if p.is_null() || frames.is_null() || out.is_null() || n < 2 {
        return -1;
    }
    let pano = &*p;
    let fr = slice::from_raw_parts(frames, n as usize);
    let prog = |f: f64| {
        if let Some(cb) = progress {
            cb(user, f);
        }
    };
    let imgs: Vec<Img> = fr.iter().map(|i| orient(&img_from_c(i), pano.exif)).collect();
    prog(0.05);
    let mut sub = |f: f64| prog(0.05 + 0.95 * f);
    match stitch(&imgs, &pano.keys, pano.focal, &mut sub) {
        Some(img) => {
            img_to_c(img, &mut *out);
            0
        }
        None => -2,
    }
}


// ---------------------------------------------------------------------------
// tests (rustc --test)

#[cfg(test)]
mod tests {
    use super::*;

    fn texture(w: usize, h: usize, seed: u32) -> Img {
        let mut img = Img::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let mut v = ((x / 12) as u32).wrapping_mul(2654435761) ^ ((y / 12) as u32).wrapping_mul(40503) ^ seed;
                v ^= v >> 13;
                v = v.wrapping_mul(0x5bd1e995);
                let base = 60 + (v >> 24) % 150;
                let o = (y * w + x) * 3;
                img.d[o] = base as u8;
                img.d[o + 1] = (base * 9 / 10) as u8;
                img.d[o + 2] = (base * 8 / 10) as u8;
            }
        }
        img
    }

    fn crop(img: &Img, x0: usize, y0: usize, w: usize, h: usize) -> Img {
        let mut o = Img::new(w, h);
        for y in 0..h {
            o.d[y * w * 3..(y + 1) * w * 3].copy_from_slice(&img.d[((y0 + y) * img.w + x0) * 3..((y0 + y) * img.w + x0 + w) * 3]);
        }
        o
    }

    #[test]
    fn orientation_roundtrip() {
        let a = texture(40, 30, 1);
        let b = orient(&a, 6);
        assert_eq!((b.w, b.h), (30, 40));
        // 6: rotate 90 cw: top-left goes to top-right
        assert_eq!(b.px(29, 0), a.px(0, 0));
        let c = orient(&b, 8);
        assert_eq!(c.d, a.d);
    }

    #[test]
    fn panorama_planar() {
        // a wide scene seen as overlapping crops (very long focal: ~planar)
        let scene = texture(1400, 300, 7);
        let (w, h) = (400usize, 260usize);
        let xs = [0usize, 250, 500, 760, 1000];
        let ys = [20usize, 22, 18, 21, 20];
        let frames: Vec<Img> = xs.iter().zip(ys.iter()).map(|(&x, &y)| crop(&scene, x, y, w, h)).collect();
        // rough tracker estimates, 10 % off
        let keys: Vec<(f64, f64)> = xs.iter().map(|&x| (x as f64 * 0.9, 0.0)).collect();
        let out = stitch(&frames, &keys, 1e6, &mut |_| {}).expect("stitch");
        assert!((out.w as i64 - 1400).abs() <= 6, "width {}", out.w);
        // compare with the scene (the crop starts at the common top row 22)
        let mut err = 0u64;
        let mut n = 0u64;
        for y in 10..out.h - 10 {
            for x in 10..out.w - 10 {
                err += (out.px(x, y)[0] as i32 - scene.px(x, y + 22)[0] as i32).unsigned_abs() as u64;
                n += 1;
            }
        }
        let mean = err as f64 / n as f64;
        assert!(mean < 4.0, "mean error {}", mean);
    }

    #[test]
    fn tracker_detects_sweep() {
        let scene = texture(2000, 200, 3);
        let mut p = Pano::new(1, 1e6, 1.0, 20);
        let mut keys = 0;
        let mut x = 0;
        while x + 300 < 2000 {
            let f = crop(&scene, x, 20, 300, 160);
            let g = Gray::from_img(&f);
            if p.feed(&g) >= 1 {
                keys += 1;
            }
            x += 13;
        }
        // ~1700 px of sweep in steps of 0.33 * 300 = 99 px
        assert!(keys >= 14 && keys <= 20, "keys {}", keys);
        assert_eq!(p.status().2, 1);
    }

}
