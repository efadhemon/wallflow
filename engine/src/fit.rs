//! Where the video goes on screen: the quad size (for letterboxing) and the part
//! of the texture shown (for cropping). Kept free of GPU types so it is testable.

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FillMode {
    /// Cover the screen, cropping the overflow from the center.
    #[default]
    Fill,
    /// Show the whole video, letterboxed.
    Fit,
    /// Distort to the screen's aspect ratio.
    Stretch,
}

impl FillMode {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "fill" => Some(Self::Fill),
            "fit" | "center" | "tile" => Some(Self::Fit),
            "stretch" => Some(Self::Stretch),
            _ => None,
        }
    }
}

/// `pos_scale` shrinks the full-screen quad (1 = edge to edge); `uv_offset` and
/// `uv_scale` select the visible part of the texture (0..1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub pos_scale: [f32; 2],
    pub uv_offset: [f32; 2],
    pub uv_scale: [f32; 2],
}

pub fn place(mode: FillMode, video: (u32, u32), screen: (u32, u32)) -> Placement {
    let video_ar = video.0.max(1) as f32 / video.1.max(1) as f32;
    let screen_ar = screen.0.max(1) as f32 / screen.1.max(1) as f32;
    let full = Placement {
        pos_scale: [1.0, 1.0],
        uv_offset: [0.0, 0.0],
        uv_scale: [1.0, 1.0],
    };

    match mode {
        FillMode::Stretch => full,
        FillMode::Fit if video_ar > screen_ar => Placement {
            pos_scale: [1.0, screen_ar / video_ar],
            ..full
        },
        FillMode::Fit => Placement {
            pos_scale: [video_ar / screen_ar, 1.0],
            ..full
        },
        FillMode::Fill if video_ar > screen_ar => {
            let visible = screen_ar / video_ar;
            Placement {
                uv_offset: [(1.0 - visible) / 2.0, 0.0],
                uv_scale: [visible, 1.0],
                ..full
            }
        }
        FillMode::Fill => {
            let visible = video_ar / screen_ar;
            Placement {
                uv_offset: [0.0, (1.0 - visible) / 2.0],
                uv_scale: [1.0, visible],
                ..full
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f32; 2], b: [f32; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4
    }

    #[test]
    fn same_aspect_is_edge_to_edge_in_every_mode() {
        for mode in [FillMode::Fill, FillMode::Fit, FillMode::Stretch] {
            let p = place(mode, (1920, 1080), (3840, 2160));
            assert!(close(p.pos_scale, [1.0, 1.0]));
            assert!(close(p.uv_scale, [1.0, 1.0]));
        }
    }

    #[test]
    fn fill_crops_a_wide_video_horizontally() {
        // 2:1 video on a 1:1 screen keeps the middle half.
        let p = place(FillMode::Fill, (200, 100), (100, 100));
        assert!(close(p.uv_offset, [0.25, 0.0]));
        assert!(close(p.uv_scale, [0.5, 1.0]));
        assert!(close(p.pos_scale, [1.0, 1.0]));
    }

    #[test]
    fn fit_letterboxes_a_tall_video() {
        // 1:2 video on a 1:1 screen uses half the width.
        let p = place(FillMode::Fit, (100, 200), (100, 100));
        assert!(close(p.pos_scale, [0.5, 1.0]));
        assert!(close(p.uv_scale, [1.0, 1.0]));
    }
}
