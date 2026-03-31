// Copyright (c) 2026 IDLab Discover
// SPDX-License-Identifier: MIT

//! YOLOv8 Detector Terminal UI
//!
//! This component provides a terminal-based user interface for real-time 
//! object detection using YOLOv8. It reads frames from a WASI-USB camera
//! stream and renders bounding boxes using Ratatui.

#[cfg(target_family = "wasm")]
mod wasm_component {
    use usb_wasm_bindings as bindings;
    use bindings::exports::wasi::cli::run::Guest as RunGuest;
    use bindings::component::usb::cv::{
        FrameStream, ObjectDetector
    };

    use ratatui::{
        backend::CrosstermBackend,
        widgets::{Block, Borders, Paragraph, canvas::{Canvas, Rectangle}},
        layout::{Layout, Constraint, Direction},
        Terminal,
        style::{Color},
    };
    use std::io;

    struct YoloTerminalDetector;

    impl RunGuest for YoloTerminalDetector {
        fn run() -> Result<(), ()> {
            let args = bindings::wasi::cli::environment::get_arguments();
            let model_path = args.get(1).map(|s| s.as_str()).unwrap_or("yolov8n.onnx");
            let camera_index = args.get(2)
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(0);

            // Initialize resources (imported from yolo-detector component via composition)
            let stream = FrameStream::new(camera_index);
            let detector = ObjectDetector::new(model_path);
            
            // Setup Terminal
            let mut stdout = io::stdout();
            crossterm::terminal::enable_raw_mode().map_err(|_| ())?;
            crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen).map_err(|_| ())?;
            let backend = CrosstermBackend::new(stdout);
            let mut terminal = Terminal::new(backend).map_err(|_| ())?;

            loop {
                let frame = stream.read_frame().map_err(|_| ())?;
                let detections = detector.detect(&frame).map_err(|_| ())?;

                terminal.draw(|f| {
                    let chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([
                            Constraint::Length(3),
                            Constraint::Min(0),
                        ].as_ref())
                        .split(f.size());

                    let title = Paragraph::new(format!("YOLO Detector TUI - Camera {} - Model {}", camera_index, model_path))
                        .block(Block::default().borders(Borders::ALL));
                    f.render_widget(title, chunks[0]);

                    let canvas = Canvas::default()
                        .block(Block::default().borders(Borders::ALL).title("Video Stream (Bounding Boxes)"))
                        .x_bounds([0.0, frame.width as f64])
                        .y_bounds([0.0, frame.height as f64])
                        .paint(|ctx| {
                            for det in &detections {
                                ctx.draw(&Rectangle {
                                    x: det.box_.origin.x as f64,
                                    y: (frame.height - det.box_.origin.y - det.box_.size.height) as f64,
                                    width: det.box_.size.width as f64,
                                    height: det.box_.size.height as f64,
                                    color: Color::Red,
                                });
                                ctx.print(
                                    det.box_.origin.x as f64,
                                    (frame.height - det.box_.origin.y) as f64,
                                    format!("{} ({:.2})", det.label, det.confidence),
                                    Color::Yellow,
                                );
                            }
                        });
                    f.render_widget(canvas, chunks[1]);
                }).map_err(|_| ())?;

                // Save annotated image to disk for verification (WASI preopen required)
                if let Some(mut img) = image::RgbImage::from_raw(frame.width, frame.height, frame.data.clone()) {
                    for det in &detections {
                        draw_hollow_rect(&mut img, det.box_.origin.x, det.box_.origin.y, det.box_.size.width, det.box_.size.height, [0, 255, 0]);
                    }
                    let _ = img.save("yolo_terminal_output.jpg");
                }

                // Check for exit key
                if crossterm::event::poll(std::time::Duration::from_millis(10)).map_err(|_| ())? {
                    if let crossterm::event::Event::Key(key) = crossterm::event::read().map_err(|_| ())? {
                        if key.code == crossterm::event::KeyCode::Char('q') {
                            break;
                        }
                    }
                }
            }

            // Cleanup
            crossterm::terminal::disable_raw_mode().map_err(|_| ())?;
            crossterm::execute!(terminal.backend_mut(), crossterm::terminal::LeaveAlternateScreen).map_err(|_| ())?;
            terminal.show_cursor().map_err(|_| ())?;

            Ok(())
        }
    }

    fn draw_hollow_rect(img: &mut image::RgbImage, x: u32, y: u32, w: u32, h: u32, color: [u8; 3]) {
        for i in 0..w {
            if x + i < img.width() {
                if y < img.height() { img.put_pixel(x + i, y, image::Rgb(color)); }
                if y + h.saturating_sub(1) < img.height() { img.put_pixel(x + i, y + h.saturating_sub(1), image::Rgb(color)); }
            }
        }
        for i in 0..h {
            if y + i < img.height() {
                if x < img.width() { img.put_pixel(x, y + i, image::Rgb(color)); }
                if x + w.saturating_sub(1) < img.width() { img.put_pixel(x + w.saturating_sub(1), y + i, image::Rgb(color)); }
            }
        }
    }

    bindings::export!(YoloTerminalDetector with_types_in bindings);
}
