use crate::text::TextSession;
use vizir_core::{FontWeight, Frame, MirScale, Point, Rect};
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

    #[allow(clippy::too_many_arguments)]
    pub fn with_categories(
        mut self,
        id: &str,
        frame: Frame,
        labels: &[String],
        x_title: Option<&str>,
        ticks: Option<&NumericTickLabels>,
        text: Option<&TextSession>,
        actual_range: Option<[f64; 2]>,
    ) -> Result<Self, String> {
        if let Some(text) = text
            && text.has_category_layout(id)
        {
            let mut anchors = self.plot;
            if let Some(range) = actual_range {
                anchors[0] = range[0];
                anchors[2] = range[1];
            }
            let allocation = text
                .category_allocation(id, frame, anchors, labels, x_title.unwrap_or_default())
                .map_err(|e| e.to_string())?
                .expect("selected category role");
            self.plot[3] = allocation.plot_bottom;
            // The reduced vertical plot must still fit every unchanged numeric tick.
            self = self.with_numeric_ticks_and_text(id, frame, x_title, ticks, Some(text))?;
            if self.plot[3] - self.plot[1] < 64. {
                return Err(format!(
                    "VIZ-LAYOUT-0006: chart {id:?} cannot fit wrapped categories and a 64px plot; compile original HIR with a larger frame"
                ));
            }
            return Ok(self);
        }
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
        let title_block = text
            .map(|text| text.chart_title_bounds(id, title, frame))
            .transpose()
            .map_err(|e| e.to_string())?
            .flatten();
        let title_width = match title_block {
            Some(block) => Some(block.width),
            None => title
                .map(|label| text_width(label, 18.0, FontWeight::Bold, text))
                .transpose()?,
        };
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

        if let Some(block) = title_block {
            return Self::with_title_block(
                id,
                frame,
                block,
                y_title,
                labels,
                text.expect("measured title"),
            );
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
    fn with_title_block(
        id: &str,
        frame: Frame,
        title: Rect,
        y_title: Option<&str>,
        labels: &[String],
        text: &TextSession,
    ) -> Result<Self, String> {
        let fail = |detail: &str| {
            format!(
                "VIZ-LAYOUT-0004: chart {id:?} {detail}; enlarge its frame or revise the explicit title layout in original HIR"
            )
        };
        let footprint = |label: &str, position: Point| -> Result<Rect, String> {
            let b = text
                .single_line_box(
                    label,
                    10.5,
                    FontWeight::Regular,
                    Point {
                        x: position.x + 13.,
                        y: position.y + 4.,
                    },
                )
                .map_err(|e| e.to_string())?;
            let x = b.x.min(position.x);
            let y = b.y.min(position.y - 5.);
            Ok(Rect {
                x,
                y,
                width: (b.x + b.width).max(position.x + 10.) - x,
                height: (b.y + b.height).max(position.y + 5.) - y,
            })
        };
        let columns = labels.len().clamp(1, 3);
        let start_x = frame.width - (columns as f64 * 78. + 18.);
        let mut compact = true;
        let mut positions = Vec::with_capacity(labels.len());
        let mut boxes = Vec::with_capacity(labels.len());
        for (i, label) in labels.iter().enumerate() {
            let p = Point {
                x: frame.x + start_x + (i % 3) as f64 * 78.,
                y: frame.y + 17. + (i / 3) as f64 * 18.,
            };
            let b = footprint(label, p)?;
            compact &= b.x >= frame.x + 18.
                && b.x + b.width <= frame.x + frame.width - 18.
                && (i % 3 == 2 || i + 1 == labels.len() || b.x + b.width + 8. <= p.x + 78.)
                && b.x >= title.x + title.width + 8.;
            positions.push(p);
            boxes.push(b);
        }
        let mut bottom = (title.y + title.height - frame.y).max(32.5);
        if !compact {
            positions.clear();
            boxes.clear();
            // Probe the unchanged single-line legend footprints to reserve their
            // real ascenders/descenders around the existing swatch-center origin.
            let metrics = labels
                .iter()
                .map(|label| footprint(label, Point { x: 0., y: 0. }))
                .collect::<Result<Vec<_>, _>>()?;
            let top = metrics.iter().map(|b| b.y).fold(-5.0, f64::min);
            let mut x = frame.x + 18.;
            let mut y = frame.y + bottom + 8. - top;
            let mut row_bottom = y + 5.;
            for (label, metric) in labels.iter().zip(&metrics) {
                if x > frame.x + 18. && x + metric.x + metric.width > frame.x + frame.width - 18. {
                    x = frame.x + 18.;
                    y = row_bottom + 8. - top;
                    row_bottom = y + 5.;
                }
                let p = Point { x, y };
                let b = footprint(label, p)?;
                if b.x < frame.x + 8. || b.x + b.width > frame.x + frame.width - 8. {
                    return Err(fail(
                        "cannot fit a complete legend entry below its wrapped title",
                    ));
                }
                row_bottom = row_bottom.max(b.y + b.height);
                x = b.x + b.width + 16.;
                positions.push(p);
                boxes.push(b);
            }
        }
        for b in boxes {
            bottom = bottom.max(b.y + b.height - frame.y);
        }
        // Reserve the actual measured y-axis title top and bottom at the same
        // existing baseline relation (plot top - 12), rather than an em proxy.
        let top = if let Some(label) = y_title {
            let b = text
                .single_line_box(
                    label,
                    12.5,
                    FontWeight::Medium,
                    Point {
                        x: frame.x + 16.,
                        y: 0.,
                    },
                )
                .map_err(|e| e.to_string())?;
            if b.y + b.height > 12. {
                return Err(fail("y-axis title descender reaches the plot"));
            }
            (bottom + 6. - b.y + 12.).max(50.)
        } else {
            (bottom + 12.).max(50.)
        };
        let plot = [
            frame.x + 64.,
            frame.y + top,
            frame.x + frame.width - 30.,
            frame.y + frame.height - 62.,
        ];
        if plot[2] - plot[0] < 64. || plot[3] - plot[1] < 64. {
            return Err(fail(
                "cannot fit its wrapped title, legend and a 64px by 64px plot",
            ));
        }
        Ok(Self {
            plot,
            legend: positions,
        })
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
