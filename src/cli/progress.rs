use std::io::Write as _;

const BAR_WIDTH: usize = 20;

pub struct ProgressBar {
    total: usize,
    current: usize,
    label: &'static str,
}

impl ProgressBar {
    pub fn new(total: usize, label: &'static str) -> Self {
        let pb = Self { total, current: 0, label };
        pb.render();
        pb
    }

    pub fn inc(&mut self, n: usize) {
        self.current = (self.current + n).min(self.total);
        self.render();
    }

    pub fn finish(&self) {
        eprintln!();
    }

    fn render(&self) {
        if self.total == 0 {
            return;
        }
        let pct = (self.current as f64 / self.total as f64 * 100.0).min(100.0);
        let filled = (pct / 100.0 * BAR_WIDTH as f64) as usize;
        let empty = BAR_WIDTH - filled;
        eprint!(
            "\r{}{}  {:.0}%  {}/{}  {}",
            "\u{2588}".repeat(filled),
            "\u{2591}".repeat(empty),
            pct,
            self.current,
            self.total,
            self.label,
        );
        std::io::stderr().flush().ok();
    }
}
