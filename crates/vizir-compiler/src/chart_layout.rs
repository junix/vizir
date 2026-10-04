use crate::text::TextSession;
use vizir_core::{FontWeight, Frame, MirScale, Point};
fn text_width(
    text: &str,
    size: f64,
    weight: FontWeight,
    session: Option<&TextSession>,
) -> Result<f64, String> {
    session.map_or_else(
        || Ok(header_text_width(text, size)),
        |s| s.width(text, size, weight).map_err(|e| e.to_string()),
    )
}

use crate::tick_format::NumericTickLabels;

/// Deterministic header allocation, shared by MIR scale resolution and Scene2D.
/// These are conservative advance estimates for the default sans-serif stack,
/// not font shaping or measured glyph bounds. Non-ASCII code points (including
/// combining marks) each reserve 1.2 em; fonts are still chosen by the backend.
pub(crate) fn header_text_width(text: &str, size: f64) -> f64 {
    4.0 + text
        .chars()
        .map(|character| match character {
            ' ' => 0.4,
            'i' | 'l' | 'I' | '!' | '|' | '.' | ',' | ':' | ';' | '\'' | '`' => 0.45,
            'm' | 'w' | 'M' | 'W' | '@' | '%' => 1.1,
            'A'..='Z' => 0.85,
            '0'..='9' => 0.75,
            'a'..='z' => 0.7,
            _ => 1.2,
        })
        .sum::<f64>()
        * size
}

pub(crate) fn legend_domain(scale: Option<&MirScale>) -> &[String] {
    match scale {
        Some(MirScale::OrdinalColor { domain, .. }) => domain,
        _ => &[],
    }
}

pub(crate) struct ChartLayout {
    pub plot: [f64; 4],
    // Swatch centers use x + 5; label baselines use (x + 13, y + 4).
    pub legend: Vec<Point>,
}

impl ChartLayout {
    /// Reserve the same conservative envelopes used for emitted numeric text.
    /// Keep legacy 0.1/absent-format charts byte-identical by opting in explicitly.
    pub fn with_numeric_ticks_and_text(
        mut self,
        id: &str,
        frame: Frame,
        x_title: Option<&str>,
        ticks: Option<&NumericTickLabels>,
        text: Option<&TextSession>,
    ) -> Result<Self, String> {
        let Some(ticks) = ticks else {
            return Ok(self);
        };
        let fail = |detail: String| {
            format!(
                "VIZ-LAYOUT-0006: chart {id:?} {detail}; enlarge its frame or choose a shorter number_format"
            )
        };
        let x_widths = ticks
            .x
            .iter()
            .map(|label| text_width(label, 11.0, FontWeight::Regular, text))
            .collect::<Result<Vec<_>, _>>()?;
        let y_width = ticks
            .y
            .iter()
            .map(|label| text_width(label, 11.0, FontWeight::Regular, text))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .fold(0.0, f64::max);
        let left = 64.0_f64
            .max(if ticks.y.is_empty() {
                0.0
            } else {
                y_width + 18.0
            })
            .max(x_widths.first().copied().unwrap_or(0.0) / 2.0 + 8.0);
        let right = 30.0_f64.max(x_widths.last().copied().unwrap_or(0.0) / 2.0 + 8.0);
        self.plot[0] = frame.x + left;
        self.plot[2] = frame.x + frame.width - right;
        let required_width = x_widths
            .windows(2)
            .map(|pair| ((pair[0] + pair[1]) / 2.0 + 8.0) * 5.0)
            .fold(64.0, f64::max);
        let required_height = if ticks.y.is_empty() {
            64.0
        } else {
            (11.0 * 1.25 + 4.0) * 5.0
        };
        let width = self.plot[2] - self.plot[0];
        let height = self.plot[3] - self.plot[1];
        if width < required_width || height < required_height {
            return Err(fail(format!(
                "numeric tick labels need a {required_width:.1}px by {required_height:.1}px plot after {left:.1}px/{right:.1}px insets, but only {width:.1}px by {height:.1}px is available"
            )));
        }
        if let Some(title) = x_title {
            let center = (self.plot[0] + self.plot[2]) / 2.0 - frame.x;
            let available = 2.0 * center.min(frame.width - center) - 16.0;
            let needed = text_width(title, 12.5, FontWeight::Medium, text)?;
            if needed > available {
                return Err(fail(format!(
                    "x-axis title needs {needed:.1}px after numeric tick allocation, but only {available:.1}px is available"
                )));
            }
        }
        Ok(self)
    }

    pub fn with_categories(
        self,
        id: &str,
        labels: &[String],
        text: Option<&TextSession>,
    ) -> Result<Self, String> {
        if let Some(text) = text {
            let step = (self.plot[2] - self.plot[0]) / labels.len().max(1) as f64;
            let size = if labels.len() > 8 { 8.2 } else { 10.0 };
            for (i, label) in labels.iter().enumerate() {
                let needed = text
                    .width(label, size, FontWeight::Regular)
                    .map_err(|e| e.to_string())?;
                if needed + 8.0 > step {
                    return Err(format!(
                        "VIZ-TEXT-0006: chart {id:?} category label {i} requires {needed:.1}px plus 8px separation, but its band is {step:.1}px; enlarge the frame or shorten the original label"
                    ));
                }
            }
        }
        Ok(self)
    }

    #[cfg(test)]
    pub fn new(
        id: &str,
        frame: Frame,
        title: Option<&str>,
        x_title: Option<&str>,
        y_title: Option<&str>,
        labels: &[String],
    ) -> Result<Self, String> {
        Self::new_with_text(id, frame, title, x_title, y_title, labels, None)
    }
    pub fn new_with_text(
        id: &str,
        frame: Frame,
        title: Option<&str>,
        x_title: Option<&str>,
        y_title: Option<&str>,
        labels: &[String],
        text: Option<&TextSession>,
    ) -> Result<Self, String> {
        let fail = |detail: String| {
            format!(
                "VIZ-LAYOUT-0004: chart {id:?} {detail}; enlarge its frame or shorten the header text"
            )
        };
        let available = frame.width - 36.0;
        let title_width = title
            .map(|label| text_width(label, 18.0, FontWeight::Bold, text))
            .transpose()?;
        for (kind, width) in title_width.map(|width| ("title", width)).into_iter().chain(
            y_title
                .map(|label| text_width(label, 12.5, FontWeight::Medium, text))
                .transpose()?
                .map(|w| ("y-axis title", w)),
        ) {
            if width > available {
                return Err(fail(format!(
                    "{kind} needs {width:.1}px, but only {available:.1}px is available"
                )));
            }
        }
        // The x-axis title is centered on the asymmetrically inset plot.
        if let Some(label) = x_title {
            let width = text_width(label, 12.5, FontWeight::Medium, text)?;
            let available = frame.width - 70.0;
            if width > available {
                return Err(fail(format!(
                    "x-axis title needs {width:.1}px, but only {available:.1}px is available"
                )));
            }
        }
        let widths = labels
            .iter()
            .map(|label| text_width(label, 10.5, FontWeight::Regular, text).map(|w| 13.0 + w))
            .collect::<Result<Vec<_>, _>>()?;
        for (index, width) in widths.iter().enumerate() {
            if *width > available {
                return Err(fail(format!(
                    "legend label {index} needs {width:.1}px, but only {available:.1}px is available"
                )));
            }
        }

        // Retain the existing compact placements only when their envelopes fit.
        // Otherwise place every legend entry in input order on width-aware rows
        // below the title. No label is truncated, hidden, or shrunk.
        let columns = labels.len().clamp(1, 3);
        let start_x = frame.width - (columns as f64 * 78.0 + 18.0);
        let compact = widths.iter().enumerate().all(|(index, width)| {
            let x = start_x + (index % 3) as f64 * 78.0;
            x >= 18.0
                && x + width <= frame.width - 18.0
                && (index % 3 == 2 || index + 1 == labels.len() || *width + 8.0 <= 78.0)
                && title_width.is_none_or(|title_width| x >= 18.0 + title_width + 8.0)
        });
        let mut legend = Vec::with_capacity(labels.len());
        let mut bottom: f64 = if title.is_some() { 32.5 } else { 0.0 };
        if compact {
            for index in 0..labels.len() {
                legend.push(Point {
                    x: frame.x + start_x + (index % 3) as f64 * 78.0,
                    y: frame.y + 17.0 + (index / 3) as f64 * 18.0,
                });
            }
        } else {
            let mut x = 18.0;
            let mut y = if title.is_some() {
                bottom + 8.0 + 6.5
            } else {
                17.0
            };
            for width in widths {
                if x > 18.0 && x + width > frame.width - 18.0 {
                    x = 18.0;
                    y += 18.0;
                }
                legend.push(Point {
                    x: frame.x + x,
                    y: frame.y + y,
                });
                x += width + 16.0;
            }
        }
        if let Some(last) = legend.last() {
            bottom = bottom.max(last.y - frame.y + 6.625);
        }
        // Text bounds extend 1 em above and 0.25 em below the baseline.
        // Keep the axis title below the entire header and above the first tick.
        let top = if y_title.is_some() {
            (bottom + 6.0 + 12.5 + 12.0).max(50.0)
        } else {
            (bottom + 12.0).max(50.0)
        };
        let plot = [
            frame.x + 64.0,
            frame.y + top,
            frame.x + frame.width - 30.0,
            frame.y + frame.height - 62.0,
        ];
        // Six ticks need a usable region, not a zero or inverted scale range.
        if plot[2] - plot[0] < 64.0 || plot[3] - plot[1] < 64.0 {
            return Err(fail(
                "cannot fit its header and a 64px by 64px plot".to_owned(),
            ));
        }
        Ok(Self { plot, legend })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_envelope_distinguishes_widths_and_reserves_non_ascii_code_points() {
        assert!(header_text_width("WWW", 10.5) > header_text_width("iii", 10.5));
        assert_eq!(header_text_width("東京", 10.0), 28.0);
        assert!(header_text_width("e\u{301}", 10.0) > header_text_width("e", 10.0));
    }

    #[test]
    fn minimum_plot_and_single_label_width_boundaries_are_inclusive() {
        let mut frame = Frame {
            x: 0.0,
            y: 0.0,
            width: 158.0,
            height: 176.0,
        };
        let layout = ChartLayout::new("test", frame, None, None, None, &[]).unwrap();
        assert_eq!(layout.plot, [64.0, 50.0, 128.0, 114.0]);
        frame.height -= 0.001;
        assert!(ChartLayout::new("test", frame, None, None, None, &[]).is_err());
        frame.height = 400.0;
        frame.width -= 0.001;
        assert!(ChartLayout::new("test", frame, None, None, None, &[]).is_err());
        let label = "WWWWWWWWWW".to_owned();
        frame.width = 36.0 + 13.0 + header_text_width(&label, 10.5);
        assert!(
            ChartLayout::new(
                "test",
                frame,
                None,
                None,
                None,
                std::slice::from_ref(&label)
            )
            .is_ok()
        );
        frame.width -= 0.001;
        assert!(ChartLayout::new("test", frame, None, None, None, &[label]).is_err());
    }
}
