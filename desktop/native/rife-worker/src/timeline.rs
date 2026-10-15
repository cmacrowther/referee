//! Frame-rate conversion timing: which input frames, and which blend position
//! between them, make up each output frame, plus the lag monitor that keeps a
//! live stream at real time when interpolation cannot keep up.

/// Blend positions this close to an input frame copy that frame instead.
const SNAP_TIMESTEP: f64 = 0.02;

/// A frame rate as an exact fraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameRate {
    pub num: u64,
    pub den: u64,
}

impl FrameRate {
    /// Parses `"60"`, `"29.97"`, or `"30000/1001"`. Decimal NTSC rates snap to
    /// their exact x·1000/1001 fraction so long streams do not drift.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        if let Some((num, den)) = text.split_once('/') {
            let (num, den) = (num.trim().parse().ok()?, den.trim().parse().ok()?);
            return Self::new(num, den);
        }
        let value: f64 = text.parse().ok()?;
        if !value.is_finite() || value <= 0.0 {
            return None;
        }
        if (value - value.round()).abs() < 1e-3 {
            return Self::new(value.round() as u64, 1);
        }
        let ntsc = value * 1.001;
        if (ntsc - ntsc.round()).abs() < 1e-2 {
            return Self::new(ntsc.round() as u64 * 1000, 1001);
        }
        Self::new((value * 1000.0).round() as u64, 1000)
    }

    fn new(num: u64, den: u64) -> Option<Self> {
        if num == 0 || den == 0 {
            return None;
        }
        let divisor = gcd(num, den);
        Some(Self {
            num: num / divisor,
            den: den / divisor,
        })
    }

    pub fn as_f64(self) -> f64 {
        self.num as f64 / self.den as f64
    }

    /// The rate as an FFmpeg argument (`"60"` or `"30000/1001"`).
    pub fn ffmpeg_arg(self) -> String {
        if self.den == 1 {
            self.num.to_string()
        } else {
            format!("{}/{}", self.num, self.den)
        }
    }
}

fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// How one output frame is made.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OutputFrame {
    /// Input frame `index` unchanged.
    Copy(u64),
    /// The frame at `timestep` (0..1) between input `from` and `from + 1`.
    Blend { from: u64, timestep: f32 },
}

impl OutputFrame {
    /// The last input frame this output needs.
    pub fn last_input(self) -> u64 {
        match self {
            Self::Copy(index) => index,
            Self::Blend { from, .. } => from + 1,
        }
    }

    /// The input frame nearest to this output, for when blending is skipped.
    pub fn nearest(self) -> u64 {
        match self {
            Self::Copy(index) => index,
            Self::Blend { from, timestep } if timestep < 0.5 => from,
            Self::Blend { from, .. } => from + 1,
        }
    }
}

/// Places output frames on the target-rate grid against input frames on the
/// source-rate grid, both starting at the first input frame.
#[derive(Debug, Clone, Copy)]
pub struct Schedule {
    source: FrameRate,
    target: FrameRate,
}

impl Schedule {
    pub fn new(source: FrameRate, target: FrameRate) -> Self {
        Self { source, target }
    }

    /// Output frame `index`, at time `index / target`.
    pub fn frame(&self, index: u64) -> OutputFrame {
        // Position in input frames: index * (1/target) / (1/source).
        let numer = index as u128 * self.source.num as u128 * self.target.den as u128;
        let denom = self.source.den as u128 * self.target.num as u128;
        let from = (numer / denom) as u64;
        let fraction = (numer % denom) as f64 / denom as f64;
        if fraction < SNAP_TIMESTEP {
            OutputFrame::Copy(from)
        } else if fraction > 1.0 - SNAP_TIMESTEP {
            OutputFrame::Copy(from + 1)
        } else {
            OutputFrame::Blend {
                from,
                timestep: fraction as f32,
            }
        }
    }

    /// Stream time of output frame `index`, in seconds.
    pub fn output_time(&self, index: u64) -> f64 {
        index as f64 / self.target.as_f64()
    }
}

/// Tracks how far output has fallen behind real time. Past `enter_seconds`
/// the worker stops blending and repeats frames until it is back within
/// `exit_seconds`, so a live stream keeps its place instead of drifting off
/// the source playlist.
#[derive(Debug, Clone, Copy)]
pub struct LagMonitor {
    enter_seconds: f64,
    exit_seconds: f64,
    behind: bool,
}

impl LagMonitor {
    pub fn new(enter_seconds: f64, exit_seconds: f64) -> Self {
        Self {
            enter_seconds,
            exit_seconds,
            behind: false,
        }
    }

    /// Updates with the output's stream time and the wall time since the
    /// first input frame; returns whether to skip blending.
    pub fn update(&mut self, stream_seconds: f64, wall_seconds: f64) -> bool {
        let lag = wall_seconds - stream_seconds;
        self.behind = if self.behind {
            lag > self.exit_seconds
        } else {
            lag > self.enter_seconds
        };
        self.behind
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rate(text: &str) -> FrameRate {
        FrameRate::parse(text).unwrap()
    }

    #[test]
    fn parses_integer_ntsc_and_fractional_rates() {
        assert_eq!(rate("60"), FrameRate { num: 60, den: 1 });
        assert_eq!(rate("60.0"), FrameRate { num: 60, den: 1 });
        assert_eq!(
            rate("29.97"),
            FrameRate {
                num: 30000,
                den: 1001
            }
        );
        assert_eq!(
            rate("23.976"),
            FrameRate {
                num: 24000,
                den: 1001
            }
        );
        assert_eq!(
            rate("59.94"),
            FrameRate {
                num: 60000,
                den: 1001
            }
        );
        assert_eq!(
            rate("30000/1001"),
            FrameRate {
                num: 30000,
                den: 1001
            }
        );
        assert_eq!(rate("12.5"), FrameRate { num: 25, den: 2 });
        assert_eq!(rate("30000/1001").ffmpeg_arg(), "30000/1001");
        assert_eq!(rate("60").ffmpeg_arg(), "60");
        assert_eq!(FrameRate::parse("0"), None);
        assert_eq!(FrameRate::parse("-5"), None);
        assert_eq!(FrameRate::parse("abc"), None);
    }

    #[test]
    fn doubling_alternates_copies_and_midpoints() {
        let schedule = Schedule::new(rate("30"), rate("60"));
        let frames: Vec<_> = (0..5).map(|index| schedule.frame(index)).collect();
        assert_eq!(
            frames,
            vec![
                OutputFrame::Copy(0),
                OutputFrame::Blend {
                    from: 0,
                    timestep: 0.5
                },
                OutputFrame::Copy(1),
                OutputFrame::Blend {
                    from: 1,
                    timestep: 0.5
                },
                OutputFrame::Copy(2),
            ]
        );
    }

    #[test]
    fn non_integer_ratios_blend_at_exact_positions() {
        // 24 -> 60 repeats every 5 output frames (2 input frames).
        let schedule = Schedule::new(rate("24"), rate("60"));
        let frames: Vec<_> = (0..6).map(|index| schedule.frame(index)).collect();
        assert_eq!(
            frames,
            vec![
                OutputFrame::Copy(0),
                OutputFrame::Blend {
                    from: 0,
                    timestep: 0.4
                },
                OutputFrame::Blend {
                    from: 0,
                    timestep: 0.8
                },
                OutputFrame::Blend {
                    from: 1,
                    timestep: 0.2
                },
                OutputFrame::Blend {
                    from: 1,
                    timestep: 0.6
                },
                OutputFrame::Copy(2),
            ]
        );
    }

    #[test]
    fn ntsc_sources_stay_on_the_exact_grid_over_long_streams() {
        // 29.97 -> 60 over ten hours: output frame n lands at n * 0.4995 input
        // frames with no floating-point drift.
        let schedule = Schedule::new(rate("29.97"), rate("60"));
        let index = 60 * 3600 * 10;
        let expected_position = index as f64 * (30000.0 / 1001.0) / 60.0;
        assert_eq!(
            schedule.frame(index).last_input(),
            expected_position.floor() as u64 + 1
        );
        assert_eq!(schedule.output_time(index), 36000.0);
    }

    #[test]
    fn blends_near_an_input_snap_to_it() {
        // 59.94 -> 60: blends drift past input frames slowly; positions within
        // 2% of a frame copy it rather than spend an inference.
        let schedule = Schedule::new(rate("59.94"), rate("60"));
        assert_eq!(schedule.frame(1), OutputFrame::Copy(1));
        assert!(matches!(schedule.frame(500), OutputFrame::Blend { .. }));
    }

    #[test]
    fn nearest_input_rounds_the_blend_position() {
        assert_eq!(
            OutputFrame::Blend {
                from: 3,
                timestep: 0.4
            }
            .nearest(),
            3
        );
        assert_eq!(
            OutputFrame::Blend {
                from: 3,
                timestep: 0.6
            }
            .nearest(),
            4
        );
        assert_eq!(OutputFrame::Copy(7).nearest(), 7);
        assert_eq!(
            OutputFrame::Blend {
                from: 3,
                timestep: 0.6
            }
            .last_input(),
            4
        );
    }

    #[test]
    fn lag_monitor_skips_blending_with_hysteresis() {
        let mut monitor = LagMonitor::new(1.0, 0.25);
        assert!(!monitor.update(5.0, 5.5));
        assert!(monitor.update(5.0, 6.5));
        // Still catching up until within the exit threshold.
        assert!(monitor.update(6.0, 6.5));
        assert!(!monitor.update(6.4, 6.5));
        // Running ahead of real time (startup burst) never skips.
        assert!(!monitor.update(9.0, 2.0));
    }
}
