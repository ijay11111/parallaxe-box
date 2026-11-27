//! 纯矢量 `egui::Painter` 图标库（零 Emoji 豆腐块，跨平台高 DPI 清晰渲染）。

use egui::{pos2, vec2, Color32, Pos2, Rect, Rounding, Sense, Stroke, Ui};

/// 内置矢量图标种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconKind {
    Brand,
    Overview,
    Services,
    Coverage,
    Guard,
    Timeline,
    Catalog,
    Settings,
    Globe,
    Layers,
    Alert,
    Play,
    Stop,
    Sun,
    Moon,
    ChevronRight,
    ChevronLeft,
    Lock,
    Close,
    Check,
    Info,
    Plugin,
    Spark,
}

/// 在给定矩形中绘制矢量图标。
pub fn paint_icon(painter: &egui::Painter, rect: Rect, kind: IconKind, color: Color32) {
    let c = rect.center();
    let r = rect.width().min(rect.height()) * 0.42;
    let stroke = Stroke::new((r * 0.22).clamp(1.3_f32, 2.2_f32), color);

    match kind {
        IconKind::Brand => {
            // 棱镜双菱形 + 中心点
            let outer = vec![
                pos2(c.x, c.y - r),
                pos2(c.x + r, c.y),
                pos2(c.x, c.y + r),
                pos2(c.x - r, c.y),
            ];
            painter.add(egui::Shape::closed_line(outer, stroke));
            painter.circle_filled(c, r * 0.24, color);
        }
        IconKind::Overview => {
            // 房屋/总览轮廓
            let roof = vec![
                pos2(c.x - r, c.y - r * 0.05),
                pos2(c.x, c.y - r * 0.95),
                pos2(c.x + r, c.y - r * 0.05),
            ];
            painter.add(egui::Shape::line(roof, stroke));
            let body = Rect::from_min_max(
                pos2(c.x - r * 0.65, c.y - r * 0.1),
                pos2(c.x + r * 0.65, c.y + r * 0.85),
            );
            painter.rect_stroke(body, Rounding::same(2.0), stroke);
        }
        IconKind::Services => {
            // 2x2 网格
            let gap = r * 0.18;
            let cell = r * 0.72;
            for (dx, dy) in [
                (-cell - gap * 0.5, -cell - gap * 0.5),
                (gap * 0.5, -cell - gap * 0.5),
                (-cell - gap * 0.5, gap * 0.5),
                (gap * 0.5, gap * 0.5),
            ] {
                let b = Rect::from_min_size(pos2(c.x + dx, c.y + dy), vec2(cell, cell));
                painter.rect_stroke(b, Rounding::same(2.0), stroke);
            }
        }
        IconKind::Coverage => {
            // 双栏清单
            let left = Rect::from_min_max(
                pos2(c.x - r * 0.9, c.y - r * 0.75),
                pos2(c.x - r * 0.12, c.y + r * 0.75),
            );
            let right = Rect::from_min_max(
                pos2(c.x + r * 0.12, c.y - r * 0.75),
                pos2(c.x + r * 0.9, c.y + r * 0.75),
            );
            painter.rect_stroke(left, Rounding::same(2.0), stroke);
            painter.rect_stroke(right, Rounding::same(2.0), stroke);
        }
        IconKind::Guard => {
            // 盾牌轮廓
            let shield = vec![
                pos2(c.x, c.y - r),
                pos2(c.x + r * 0.82, c.y - r * 0.55),
                pos2(c.x + r * 0.68, c.y + r * 0.35),
                pos2(c.x, c.y + r),
                pos2(c.x - r * 0.68, c.y + r * 0.35),
                pos2(c.x - r * 0.82, c.y - r * 0.55),
            ];
            painter.add(egui::Shape::closed_line(shield, stroke));
        }
        IconKind::Timeline => {
            // 脉搏折线
            let wave = vec![
                pos2(c.x - r, c.y),
                pos2(c.x - r * 0.45, c.y),
                pos2(c.x - r * 0.2, c.y - r * 0.75),
                pos2(c.x + r * 0.18, c.y + r * 0.7),
                pos2(c.x + r * 0.48, c.y),
                pos2(c.x + r, c.y),
            ];
            painter.add(egui::Shape::line(wave, stroke));
        }
        IconKind::Catalog => {
            // 三行列表树
            for dy in [-r * 0.6, 0.0, r * 0.6] {
                painter.circle_filled(pos2(c.x - r * 0.75, c.y + dy), r * 0.14, color);
                painter.line_segment(
                    [
                        pos2(c.x - r * 0.35, c.y + dy),
                        pos2(c.x + r * 0.85, c.y + dy),
                    ],
                    stroke,
                );
            }
        }
        IconKind::Settings => {
            // 齿轮外圆 + 内圆
            painter.circle_stroke(c, r * 0.72, stroke);
            painter.circle_filled(c, r * 0.26, color);
            for i in 0..4 {
                let a = (i as f32) * std::f32::consts::FRAC_PI_2;
                let (s, cs) = a.sin_cos();
                painter.line_segment(
                    [
                        pos2(c.x + cs * r * 0.72, c.y + s * r * 0.72),
                        pos2(c.x + cs * r * 1.05, c.y + s * r * 1.05),
                    ],
                    stroke,
                );
            }
        }
        IconKind::Globe => {
            // 地球仪（圆 + 纬线 + 经线）
            painter.circle_stroke(c, r * 0.92, stroke);
            painter.line_segment(
                [pos2(c.x - r * 0.92, c.y), pos2(c.x + r * 0.92, c.y)],
                stroke,
            );
            painter.line_segment(
                [pos2(c.x, c.y - r * 0.92), pos2(c.x, c.y + r * 0.92)],
                stroke,
            );
            let arc_l = vec![
                pos2(c.x - r * 0.1, c.y - r * 0.88),
                pos2(c.x - r * 0.48, c.y),
                pos2(c.x - r * 0.1, c.y + r * 0.88),
            ];
            let arc_r = vec![
                pos2(c.x + r * 0.1, c.y - r * 0.88),
                pos2(c.x + r * 0.48, c.y),
                pos2(c.x + r * 0.1, c.y + r * 0.88),
            ];
            painter.add(egui::Shape::line(arc_l, stroke));
            painter.add(egui::Shape::line(arc_r, stroke));
        }
        IconKind::Layers => {
            // 堆叠三层菱形（与参考图「重要服务」一致）
            let top = vec![
                pos2(c.x, c.y - r * 0.85),
                pos2(c.x + r * 0.9, c.y - r * 0.35),
                pos2(c.x, c.y + r * 0.12),
                pos2(c.x - r * 0.9, c.y - r * 0.35),
            ];
            painter.add(egui::Shape::closed_line(top, stroke));
            let mid = vec![
                pos2(c.x - r * 0.85, c.y + r * 0.12),
                pos2(c.x, c.y + r * 0.52),
                pos2(c.x + r * 0.85, c.y + r * 0.12),
            ];
            let bot = vec![
                pos2(c.x - r * 0.85, c.y + r * 0.52),
                pos2(c.x, c.y + r * 0.92),
                pos2(c.x + r * 0.85, c.y + r * 0.52),
            ];
            painter.add(egui::Shape::line(mid, stroke));
            painter.add(egui::Shape::line(bot, stroke));
        }
        IconKind::Alert => {
            // 警告三角形 + 叹号
            let tri = vec![
                pos2(c.x, c.y - r * 0.9),
                pos2(c.x + r * 0.92, c.y + r * 0.78),
                pos2(c.x - r * 0.92, c.y + r * 0.78),
            ];
            painter.add(egui::Shape::closed_line(tri, stroke));
            painter.line_segment(
                [pos2(c.x, c.y - r * 0.32), pos2(c.x, c.y + r * 0.22)],
                stroke,
            );
            painter.circle_filled(pos2(c.x, c.y + r * 0.5), r * 0.11, color);
        }
        IconKind::Play => {
            let tri = vec![
                pos2(c.x - r * 0.45, c.y - r * 0.72),
                pos2(c.x + r * 0.75, c.y),
                pos2(c.x - r * 0.45, c.y + r * 0.72),
            ];
            painter.add(egui::Shape::convex_polygon(tri, color, Stroke::NONE));
        }
        IconKind::Stop => {
            let sq = Rect::from_center_size(c, vec2(r * 1.25, r * 1.25));
            painter.rect_filled(sq, Rounding::same(2.0), color);
        }
        IconKind::Sun => {
            painter.circle_stroke(c, r * 0.46, stroke);
            for i in 0..8 {
                let a = (i as f32) * std::f32::consts::FRAC_PI_4;
                let (s, cs) = a.sin_cos();
                painter.line_segment(
                    [
                        pos2(c.x + cs * r * 0.68, c.y + s * r * 0.68),
                        pos2(c.x + cs * r * 0.98, c.y + s * r * 0.98),
                    ],
                    stroke,
                );
            }
        }
        IconKind::Moon => {
            let arc = vec![
                pos2(c.x + r * 0.15, c.y - r * 0.82),
                pos2(c.x - r * 0.45, c.y - r * 0.55),
                pos2(c.x - r * 0.68, c.y),
                pos2(c.x - r * 0.42, c.y + r * 0.62),
                pos2(c.x + r * 0.25, c.y + r * 0.8),
                pos2(c.x + r * 0.72, c.y + r * 0.45),
                pos2(c.x + r * 0.1, c.y + r * 0.35),
                pos2(c.x - r * 0.12, c.y - r * 0.1),
                pos2(c.x + r * 0.15, c.y - r * 0.82),
            ];
            painter.add(egui::Shape::line(arc, stroke));
        }
        IconKind::ChevronRight => {
            let pts = vec![
                pos2(c.x - r * 0.35, c.y - r * 0.65),
                pos2(c.x + r * 0.4, c.y),
                pos2(c.x - r * 0.35, c.y + r * 0.65),
            ];
            painter.add(egui::Shape::line(pts, stroke));
        }
        IconKind::ChevronLeft => {
            let pts = vec![
                pos2(c.x + r * 0.35, c.y - r * 0.65),
                pos2(c.x - r * 0.4, c.y),
                pos2(c.x + r * 0.35, c.y + r * 0.65),
            ];
            painter.add(egui::Shape::line(pts, stroke));
        }
        IconKind::Lock => {
            let body = Rect::from_min_max(
                pos2(c.x - r * 0.68, c.y - r * 0.05),
                pos2(c.x + r * 0.68, c.y + r * 0.85),
            );
            painter.rect_stroke(body, Rounding::same(2.0), stroke);
            let shackle = vec![
                pos2(c.x - r * 0.4, c.y - r * 0.05),
                pos2(c.x - r * 0.4, c.y - r * 0.6),
                pos2(c.x, c.y - r * 0.88),
                pos2(c.x + r * 0.4, c.y - r * 0.6),
                pos2(c.x + r * 0.4, c.y - r * 0.05),
            ];
            painter.add(egui::Shape::line(shackle, stroke));
        }
        IconKind::Close => {
            let d = r * 0.62;
            painter.line_segment([pos2(c.x - d, c.y - d), pos2(c.x + d, c.y + d)], stroke);
            painter.line_segment([pos2(c.x + d, c.y - d), pos2(c.x - d, c.y + d)], stroke);
        }
        IconKind::Check => {
            let pts: Vec<Pos2> = vec![
                pos2(c.x - r * 0.72, c.y + r * 0.02),
                pos2(c.x - r * 0.18, c.y + r * 0.58),
                pos2(c.x + r * 0.78, c.y - r * 0.55),
            ];
            painter.add(egui::Shape::line(pts, stroke));
        }
        IconKind::Info => {
            painter.circle_stroke(c, r * 0.92, stroke);
            painter.circle_filled(pos2(c.x, c.y - r * 0.42), (r * 0.14).max(1.2), color);
            painter.line_segment(
                [pos2(c.x, c.y - r * 0.1), pos2(c.x, c.y + r * 0.48)],
                stroke,
            );
        }
        IconKind::Spark => {
            // 四角星（AI/智能）+ 右侧小点
            let star = vec![
                pos2(c.x - r * 0.15, c.y - r),
                pos2(c.x + r * 0.18, c.y - r * 0.18),
                pos2(c.x + r * 0.95, c.y + r * 0.05),
                pos2(c.x + r * 0.18, c.y + r * 0.28),
                pos2(c.x - r * 0.05, c.y + r * 0.95),
                pos2(c.x - r * 0.32, c.y + r * 0.28),
                pos2(c.x - r * 0.95, c.y + r * 0.02),
                pos2(c.x - r * 0.32, c.y - r * 0.2),
            ];
            painter.add(egui::Shape::closed_line(star, stroke));
            painter.circle_filled(pos2(c.x + r * 0.62, c.y - r * 0.62), r * 0.12, color);
        }
        IconKind::Plugin => {
            // 模块/插件图标：主方块 + 顶部/右侧接口凸点
            let body = Rect::from_min_max(
                pos2(c.x - r * 0.72, c.y - r * 0.42),
                pos2(c.x + r * 0.42, c.y + r * 0.72),
            );
            painter.rect_stroke(body, Rounding::same(2.0), stroke);
            painter.circle_stroke(pos2(c.x - r * 0.15, c.y - r * 0.68), r * 0.24, stroke);
            painter.circle_stroke(pos2(c.x + r * 0.68, c.y + r * 0.15), r * 0.24, stroke);
        }
    }
}

/// 在布局流中分配一块固定尺寸并绘制纯矢量图标。
pub fn icon(ui: &mut Ui, kind: IconKind, size: f32, color: Color32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    if ui.is_rect_visible(rect) {
        paint_icon(ui.painter(), rect, kind, color);
    }
    response
}

/// 绘制带圆角柔底的矢量图标容器（参考图 -8.png / -4.png 风格）。
pub fn icon_tile(ui: &mut Ui, kind: IconKind, size: f32, fg: Color32, bg: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.painter()
            .rect_filled(rect, Rounding::same(size * 0.25), bg);
        let inner = rect.shrink(size * 0.2);
        paint_icon(ui.painter(), inner, kind, fg);
    }
}
