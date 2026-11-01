use image::RgbaImage;

pub struct Shot {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub fn capture() -> Result<RgbaImage, String> {
    let monitors = xcap::Monitor::all().map_err(|e| e.to_string())?;
    let mut shots = Vec::with_capacity(monitors.len());
    for m in &monitors {
        let x = m.x().map_err(|e| e.to_string())?;
        let y = m.y().map_err(|e| e.to_string())?;
        let img = m.capture_image().map_err(|e| e.to_string())?;
        let (width, height) = img.dimensions();
        shots.push(Shot {
            x,
            y,
            width,
            height,
            rgba: img.into_raw(),
        });
    }
    stitch(shots).ok_or_else(|| "no monitors".to_string())
}

pub fn stitch(shots: Vec<Shot>) -> Option<RgbaImage> {
    let mut shots: Vec<Shot> = shots
        .into_iter()
        .filter(|s| {
            s.width > 0 && s.height > 0 && s.rgba.len() >= s.width as usize * s.height as usize * 4
        })
        .collect();
    if shots.is_empty() {
        return None;
    }
    if shots.len() == 1 || shots.iter().all(|s| s.x == 0 && s.y == 0) {
        let best = shots
            .into_iter()
            .max_by_key(|s| u64::from(s.width) * u64::from(s.height))?;
        return to_image(best);
    }
    let ox = shots.iter().map(|s| s.x).min()?;
    let oy = shots.iter().map(|s| s.y).min()?;
    let w = shots.iter().map(|s| i64::from(s.x) + i64::from(s.width)).max()? - i64::from(ox);
    let h = shots.iter().map(|s| i64::from(s.y) + i64::from(s.height)).max()? - i64::from(oy);
    let mut out = RgbaImage::new(u32::try_from(w).ok()?, u32::try_from(h).ok()?);
    for s in shots.drain(..) {
        let (dx, dy) = (i64::from(s.x - ox), i64::from(s.y - oy));
        let img = to_image(s)?;
        image::imageops::replace(&mut out, &img, dx, dy);
    }
    Some(out)
}

fn to_image(mut s: Shot) -> Option<RgbaImage> {
    s.rgba.truncate(s.width as usize * s.height as usize * 4);
    RgbaImage::from_raw(s.width, s.height, s.rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shot(x: i32, y: i32, w: u32, h: u32, px: [u8; 4]) -> Shot {
        Shot {
            x,
            y,
            width: w,
            height: h,
            rgba: px.repeat((w * h) as usize),
        }
    }

    #[test]
    fn stitches_side_by_side() {
        let second = [0, 255, 0, 255];
        let img = stitch(vec![shot(0, 0, 1, 1, [255, 0, 0, 255]), shot(10, 0, 1, 1, second)]).unwrap();
        assert_eq!(img.dimensions(), (10 + 1, 1));
        assert_eq!(img.get_pixel(10, 0).0, second);
        assert_eq!(img.get_pixel(0, 0).0, [255, 0, 0, 255]);
    }

    #[test]
    fn all_at_origin_keeps_largest() {
        let img = stitch(vec![shot(0, 0, 2, 2, [1, 1, 1, 255]), shot(0, 0, 3, 2, [2, 2, 2, 255])]).unwrap();
        assert_eq!(img.dimensions(), (3, 2));
        assert_eq!(img.get_pixel(0, 0).0, [2, 2, 2, 255]);
    }

    #[test]
    fn zero_sized_dropped() {
        assert!(stitch(vec![shot(0, 0, 0, 5, [0; 4])]).is_none());
    }
}
