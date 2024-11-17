use std::{
    io::Cursor,
    ops::RangeInclusive,
    path::PathBuf,
    sync::{Arc, LazyLock},
    thread,
    time::Duration,
};

use crossbeam::atomic::AtomicCell;
use eframe::{
    egui::{self, Image, Ui},
    App,
};
use enigo::{Enigo, Mouse, Settings};
use image::{imageops::FilterType, GenericImageView};
use imageproc::{
    contours::{self, Contour},
    edges,
};
use nanoid::nanoid;
use parking_lot::RwLock;
use rfd::FileDialog;
use windows::Win32::UI::{
    Input::KeyboardAndMouse::{GetAsyncKeyState, VK_F1, VK_F2},
    WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN},
};

pub static STATE: AtomicCell<State> = AtomicCell::new(State::Stop);
pub static DRAWING: AtomicCell<bool> = AtomicCell::new(false);
pub static SCREEN: LazyLock<(i32, i32)> =
    LazyLock::new(|| unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) });

#[derive(Debug, Clone, Copy)]
pub enum State {
    Drawing,
    Stop,
}

pub trait Draw {
    fn ui(&mut self, ui: &mut Ui);
}

#[derive(Debug, Clone)]
pub struct Panel {
    pub canny_value: u32,
    pub canny_image: Arc<RwLock<Option<Img>>>,
    pub lines: Arc<RwLock<Option<Vec<Contour<i32>>>>>,
    pub path: Arc<RwLock<Option<PathBuf>>>,
}

#[derive(Debug, Clone)]
pub struct Img {
    id: String,
    buf: Vec<u8>,
}

impl Default for Panel {
    fn default() -> Self {
        Self {
            canny_value: 25,
            canny_image: Arc::new(RwLock::new(None)),
            lines: Arc::new(RwLock::new(None)),
            path: Arc::new(RwLock::new(None)),
        }
    }
}

impl Panel {
    fn open_image(&self) {
        let canny_value = self.canny_value;
        let canny_image = self.canny_image.clone();
        let lines = self.lines.clone();
        let image_path = self.path.clone();
        rayon::spawn(move || {
            let Some(path) = FileDialog::new()
                .add_filter("Image", &["jpg", "jpeg", "png"])
                .pick_file()
            else {
                return;
            };
            image_path.write().replace(path.to_path_buf());

            let Ok(mut image) = image::open(&path) else {
                return;
            };
            let dim = image.dimensions();

            let center = (SCREEN.0 / 4, SCREEN.1 / 4);

            if dim.0 > (SCREEN.0 / 2) as _ && dim.0 >= dim.1 {
                image = image.resize(
                    (SCREEN.0 / 2) as _,
                    (SCREEN.1 / 2) as _,
                    FilterType::Lanczos3,
                );
            } else if dim.1 > (SCREEN.1 / 2) as _ && dim.1 >= dim.0 {
                image = image.resize(
                    ((SCREEN.1 / 2).pow(2) / (SCREEN.0 / 2)) as _,
                    (SCREEN.1 / 2) as _,
                    FilterType::Lanczos3,
                );
            }

            let image = image.to_luma8();
            let canny = edges::canny(&image, canny_value as f32, 3.0 * canny_value as f32);
            let mut data = Cursor::new(vec![]);
            canny.write_to(&mut data, image::ImageFormat::Png).ok();
            canny_image.write().replace(Img {
                id: nanoid!(),
                buf: data.into_inner(),
            });

            let mut contours = contours::find_contours(&canny);
            contours.iter_mut().for_each(|contour| {
                contour.points.iter_mut().for_each(|point| {
                    point.x += center.0;
                    point.y += center.1;
                });
            });
            lines.write().replace(contours);
        });
    }

    fn reload(&self) {
        let path = self.path.read();
        let Some(path) = path.as_ref() else {
            return;
        };
        let Ok(mut image) = image::open(path) else {
            return;
        };
        let dim = image.dimensions();

        let center = (SCREEN.0 / 4, SCREEN.1 / 4);

        if dim.0 > (SCREEN.0 / 2) as _ && dim.0 >= dim.1 {
            image = image.resize(
                (SCREEN.0 / 2) as _,
                (SCREEN.1 / 2) as _,
                FilterType::Lanczos3,
            );
        } else if dim.1 > (SCREEN.1 / 2) as _ && dim.1 >= dim.0 {
            image = image.resize(
                ((SCREEN.1 / 2).pow(2) / (SCREEN.0 / 2)) as _,
                (SCREEN.1 / 2) as _,
                FilterType::Lanczos3,
            );
        }

        let image = image.to_luma8();
        let canny = edges::canny(
            &image,
            self.canny_value as f32,
            3.0 * self.canny_value as f32,
        );
        let mut data = Cursor::new(vec![]);
        canny.write_to(&mut data, image::ImageFormat::Png).ok();
        self.canny_image.write().replace(Img {
            id: nanoid!(),
            buf: data.into_inner(),
        });

        let mut contours = contours::find_contours(&canny);
        contours.iter_mut().for_each(|contour| {
            contour.points.iter_mut().for_each(|point| {
                point.x += center.0;
                point.y += center.1;
            });
        });
        self.lines.write().replace(contours);
    }

    fn draw(&self) {
        let contours = self.lines.clone();
        rayon::spawn(move || {
            STATE.store(State::Drawing);
            DRAWING.store(true);
            let contours = contours.read();
            let Some(contours) = contours.as_ref() else {
                STATE.store(State::Stop);
                return;
            };

            let mut enigo = Enigo::new(&Settings::default()).unwrap();

            for contour in contours.iter() {
                if let State::Stop = STATE.load() {
                    enigo
                        .button(enigo::Button::Left, enigo::Direction::Release)
                        .ok();
                    break;
                }
                if contour.points.is_empty() {
                    continue;
                }

                enigo
                    .move_mouse(
                        contour.points[0].x,
                        contour.points[0].y,
                        enigo::Coordinate::Abs,
                    )
                    .ok();
                enigo
                    .button(enigo::Button::Left, enigo::Direction::Press)
                    .ok();
                for point in &contour.points[1..] {
                    if let State::Stop = STATE.load() {
                        break;
                    }
                    enigo
                        .move_mouse(point.x, point.y, enigo::Coordinate::Abs)
                        .ok();
                    thread::sleep(Duration::from_micros(100));
                }
                enigo
                    .button(enigo::Button::Left, enigo::Direction::Release)
                    .ok();
                thread::sleep(Duration::from_millis(100));
            }
            STATE.store(State::Stop);
            DRAWING.store(false);
        });
    }
}

impl Draw for Panel {
    fn ui(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if ui.button("Select Image").clicked() {
                self.open_image();
            }
            if ui
                .add(
                    egui::DragValue::new(&mut self.canny_value)
                        .range(RangeInclusive::new(1, u32::MAX)),
                )
                .changed()
            {
                self.reload();
            }
        });
        ui.separator();
        ui.label("Press F1 to start draw");
        ui.label("Press F2 to stop draw");
        ui.separator();

        if let Some(image) = self.canny_image.read().as_ref() {
            ui.add(Image::from_bytes(image.id.to_string(), image.buf.to_vec()));
        }

        if is_pressed(VK_F1.0) {
            match STATE.load() {
                State::Stop => {
                    if !DRAWING.load() {
                        self.draw();
                    }
                }
                State::Drawing => {}
            }
        }
        if is_pressed(VK_F2.0) {
            STATE.store(State::Stop);
        }
    }
}

impl App for Panel {
    fn update(&mut self, ctx: &eframe::egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint();
        egui::CentralPanel::default().show(ctx, |ui| self.ui(ui));
    }
}

pub fn is_pressed(vk: u16) -> bool {
    let status = unsafe { GetAsyncKeyState(vk as i32) as u32 };
    status >> 31 == 1
}
