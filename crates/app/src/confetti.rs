//! A short celebration when a title is completed.

use eframe::egui::{self, Color32, LayerId, Order, Pos2, Rect, Shape, Vec2, vec2};

struct Piece {
    pos: Pos2,
    vel: Vec2,
    spin: f32,
    angle: f32,
    size: f32,
    color: Color32,
}

#[derive(Default)]
pub struct Confetti {
    pieces: Vec<Piece>,
    age: f32,
    seed: u32,
}

const LIFE: f32 = 2.6;

impl Confetti {
    fn random(&mut self) -> f32 {
        // xorshift: good enough for decoration.
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        self.seed as f32 / u32::MAX as f32
    }

    pub fn fire(&mut self, area: Rect, colors: &[Color32]) {
        self.seed = (self.seed ^ 0x9E37_79B9).max(1);
        self.age = 0.0;
        self.pieces.clear();
        for i in 0..140 {
            let side = if i % 2 == 0 { area.left() } else { area.right() };
            let dir = if i % 2 == 0 { 1.0 } else { -1.0 };
            let vel = vec2(dir * (120.0 + 520.0 * self.random()), -(380.0 + 520.0 * self.random()));
            let piece = Piece {
                pos: egui::pos2(side, area.bottom() - area.height() * 0.2),
                vel,
                spin: (self.random() - 0.5) * 14.0,
                angle: self.random() * 6.0,
                size: 5.0 + 5.0 * self.random(),
                color: colors[i % colors.len().max(1)],
            };
            self.pieces.push(piece);
        }
    }

    pub fn active(&self) -> bool {
        !self.pieces.is_empty()
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        if self.pieces.is_empty() {
            return;
        }
        let dt = ctx.input(|i| i.stable_dt).min(0.05);
        self.age += dt;
        if self.age > LIFE {
            self.pieces.clear();
            return;
        }
        let fade = ((LIFE - self.age) / 0.6).min(1.0);
        let painter = ctx.layer_painter(LayerId::new(Order::Tooltip, egui::Id::unique("confetti")));
        for p in &mut self.pieces {
            p.vel.y += 900.0 * dt;
            p.vel *= 1.0 - 0.7 * dt;
            p.pos += p.vel * dt;
            p.angle += p.spin * dt;
            let (s, c) = p.angle.sin_cos();
            let (a, b) = (vec2(c, s) * p.size, vec2(-s, c) * p.size * 0.5);
            let pts = vec![p.pos - a - b, p.pos + a - b, p.pos + a + b, p.pos - a + b];
            painter.add(Shape::convex_polygon(pts, p.color.gamma_multiply(fade), egui::Stroke::NONE));
        }
        ctx.request_repaint();
    }
}
