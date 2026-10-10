use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Mode {
    #[default]
    Continuous,
    Single,
    Spread,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Zoom {
    #[default]
    FitPage,
    FitWidth,
    Custom(f64),
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Recent {
    pub path: String,
    pub page: usize,
    pub mode: Mode,
    pub zoom: Zoom,
    pub rotation: i32,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub dark: bool,
    pub recent: Vec<Recent>,
}
#[derive(Clone, Copy, Debug)]
pub struct PageRect {
    pub page: usize,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}
pub fn layout(
    sizes: &[(f64, f64)],
    page: usize,
    mode: Mode,
    zoom: Zoom,
    rotation: i32,
    width: f64,
    height: f64,
) -> (Vec<PageRect>, f64) {
    if sizes.is_empty() {
        return (vec![], 0.);
    }
    let page = page.min(sizes.len() - 1);
    let size = |i: usize| {
        let (w, h) = sizes[i];
        if rotation % 2 != 0 {
            (h, w)
        } else {
            (w, h)
        }
    };
    // Fit modes must not change scale as scrolling changes the current page.
    let (w, h) = if mode == Mode::Continuous {
        (0..sizes.len())
            .map(size)
            .fold((0.0_f64, 0.0_f64), |a, b| (a.0.max(b.0), a.1.max(b.1)))
    } else if mode == Mode::Spread {
        let start = page / 2 * 2;
        (start..(start + 2).min(sizes.len()))
            .map(size)
            .fold((0.0_f64, 0.0_f64), |a, b| (a.0.max(b.0), a.1.max(b.1)))
    } else {
        size(page)
    };
    let columns = if mode == Mode::Spread { 2. } else { 1. };
    let fit = ((width - 48. - (columns - 1.) * 20.) / columns / w).max(0.05);
    let scale = match zoom {
        Zoom::FitWidth => fit,
        Zoom::FitPage => fit.min((height - 48.) / h).max(0.05),
        Zoom::Custom(z) => z.clamp(0.2, 4.),
    };
    let range = match mode {
        Mode::Continuous => 0..sizes.len(),
        Mode::Single => page..page + 1,
        Mode::Spread => {
            let start = page / 2 * 2;
            start..(start + 2).min(sizes.len())
        }
    };
    let mut result = vec![];
    let mut y = 24.;
    let total_w = range.clone().map(|i| size(i).0 * scale).sum::<f64>()
        + (range.len().saturating_sub(1) as f64) * 20.;
    let mut x = ((width - total_w) / 2.).max(24.);
    for i in range {
        let (w, h) = size(i);
        let (w, h) = (w * scale, h * scale);
        let px = if mode == Mode::Spread {
            x
        } else {
            ((width - w) / 2.).max(24.)
        };
        let py = if mode == Mode::Continuous {
            y
        } else {
            ((height - h) / 2.).max(24.)
        };
        result.push(PageRect {
            page: i,
            x: px,
            y: py,
            w,
            h,
        });
        if mode == Mode::Spread {
            x += w + 20.;
            y = y.max(h + 48.);
        } else {
            y += h + 24.;
        }
    }
    (result, y.max(height))
}
pub fn clamp_scroll(value: f64, content: f64, viewport: f64) -> f64 {
    value.clamp(0., (content - viewport).max(0.))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn theme_settings_are_backward_compatible() {
        let mut settings: Settings = serde_json::from_str(r#"{"recent":[]}"#).unwrap();
        assert!(!settings.dark);
        settings.dark = true;
        let saved = serde_json::to_string(&settings).unwrap();
        assert!(serde_json::from_str::<Settings>(&saved).unwrap().dark);
    }
    #[test]
    fn mixed_page_scroll_has_stable_geometry() {
        let sizes = [(600., 800.), (800., 600.), (400., 900.)];
        let first = layout(&sizes, 0, Mode::Continuous, Zoom::FitPage, 0, 1000., 700.);
        let last = layout(&sizes, 2, Mode::Continuous, Zoom::FitPage, 0, 1000., 700.);
        assert_eq!(first.0[1].y, last.0[1].y);
        assert_eq!(first.0[1].w, last.0[1].w);
    }
    #[test]
    fn empty() {
        assert!(layout(&[], 0, Mode::Spread, Zoom::FitPage, 0, 1000., 700.)
            .0
            .is_empty());
    }
    #[test]
    fn spread_last_odd_page() {
        let r = layout(
            &[(600., 800.); 5],
            4,
            Mode::Spread,
            Zoom::FitPage,
            0,
            1000.,
            700.,
        )
        .0;
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].page, 4);
    }
    #[test]
    fn continuous_and_rotation() {
        let r = layout(
            &[(600., 800.); 3],
            0,
            Mode::Continuous,
            Zoom::Custom(1.),
            1,
            1000.,
            700.,
        )
        .0;
        assert_eq!(r[0].w, 800.);
        assert_eq!(r[0].h, 600.);
        assert!(r[2].y > r[1].y + r[1].h);
    }
    #[test]
    fn bounds() {
        assert_eq!(clamp_scroll(200., 50., 500.), 0.);
        assert_eq!(clamp_scroll(-5., 900., 500.), 0.);
        assert_eq!(clamp_scroll(900., 900., 500.), 400.);
    }
}
