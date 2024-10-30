use eframe::egui;
use eframe::App;
use std::sync::{Arc, Mutex};
use tokio::runtime::Runtime;
use std::path::PathBuf;
use crate::HotmartDownloader;
use rfd::FileDialog;
use std::process::Command;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

pub struct HotmartGui {
    url_input: String,
    status: Arc<Mutex<String>>,
    progress: Arc<Mutex<f32>>,
    is_downloading: Arc<Mutex<bool>>,
    download_complete: Arc<Mutex<bool>>,
    rt: Runtime,
    save_path: Option<PathBuf>,
}

impl Default for HotmartGui {
    fn default() -> Self {
        Self {
            url_input: String::new(),
            status: Arc::new(Mutex::new("Ready".to_string())),
            progress: Arc::new(Mutex::new(0.0)),
            is_downloading: Arc::new(Mutex::new(false)),
            download_complete: Arc::new(Mutex::new(false)),
            rt: Runtime::new().expect("Failed to create Tokio runtime"),
            save_path: None,
        }
    }
}

impl HotmartGui {
    fn open_video(&self) {
        if let Some(path) = &self.save_path {
            #[cfg(target_os = "windows")]
            {
                // On Windows, use the CREATE_NO_WINDOW flag to prevent command prompt from flashing
                const CREATE_NO_WINDOW: u32 = 0x08000000;
                Command::new("cmd")
                    .args(["/C", "start", "", path.to_str().unwrap_or("")])
                    .creation_flags(CREATE_NO_WINDOW)
                    .spawn()
                    .ok();
            }

            #[cfg(target_os = "macos")]
            {
                Command::new("open")
                    .arg(path)
                    .spawn()
                    .ok();
            }

            #[cfg(target_os = "linux")]
            {
                Command::new("xdg-open")
                    .arg(path)
                    .spawn()
                    .ok();
            }
        }
    }
}

impl eframe::App for HotmartGui {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.heading("Hotmart Video Downloader");

                // URL input
                ui.add_space(20.0);
                ui.horizontal(|ui| {
                    ui.label("Video URL:");
                    ui.text_edit_singleline(&mut self.url_input);
                });

                // Save location
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.label("Save location:");
                    if let Some(path) = &self.save_path {
                        ui.label(path.to_string_lossy().to_string());
                    } else {
                        ui.label("Not selected");
                    }
                    if ui.button("Browse").clicked() {
                        if let Some(path) = FileDialog::new()
                            .set_title("Save video as")
                            .add_filter("MP4 video", &["mp4"])
                            .save_file() {
                            self.save_path = Some(path);
                        }
                    }
                });

                // Download button
                ui.add_space(20.0);
                let is_downloading = *self.is_downloading.lock().unwrap();
                if ui.add_enabled(
                    !is_downloading && !self.url_input.is_empty() && self.save_path.is_some(),
                    egui::Button::new("Download")
                ).clicked() {
                    let url = self.url_input.clone();
                    let progress = self.progress.clone();
                    let is_downloading = self.is_downloading.clone();
                    let save_path = self.save_path.clone().unwrap();
                    let download_complete = self.download_complete.clone();

                    // Clone status Arc for both closures
                    let status_for_progress = self.status.clone();
                    let status_for_completion = self.status.clone();

                    *self.is_downloading.lock().unwrap() = true;
                    *self.download_complete.lock().unwrap() = false;

                    self.rt.spawn(async move {
                        match HotmartDownloader::new() {
                            Ok(downloader) => {
                                let result = downloader.download_video_with_progress_and_path(
                                    &url,
                                    save_path,
                                    move |current, total| {
                                        if total > 0 {
                                            *progress.lock().unwrap() = current as f32 / total as f32;
                                            *status_for_progress.lock().unwrap() = format!(
                                                "Downloading... {}/{}",
                                                current,
                                                total
                                            );
                                        }
                                    },
                                ).await;

                                match result {
                                    Ok(_) => {
                                        *status_for_completion.lock().unwrap() = "Download complete!".to_string();
                                        *download_complete.lock().unwrap() = true;
                                    }
                                    Err(e) => {
                                        *status_for_completion.lock().unwrap() = format!("Error: {}", e);
                                        *download_complete.lock().unwrap() = false;
                                    }
                                }
                            }
                            Err(e) => {
                                *status_for_completion.lock().unwrap() = format!("Failed to initialize downloader: {}", e);
                                *download_complete.lock().unwrap() = false;
                            }
                        }
                        *is_downloading.lock().unwrap() = false;
                    });
                }

                // Progress bar
                ui.add_space(20.0);
                let progress = *self.progress.lock().unwrap();
                let status = self.status.lock().unwrap().clone();

                ui.add(
                    egui::ProgressBar::new(progress)
                        .show_percentage()
                        .desired_width(ui.available_width())
                );

                // Status message
                ui.add_space(10.0);
                ui.label(&status);

                // Open Video button - only show when download is complete
                if *self.download_complete.lock().unwrap() {
                    ui.add_space(10.0);
                    if ui.button("Open Video").clicked() {
                        self.open_video();
                    }
                }
            });
        });

        // Request repaint if downloading
        if *self.is_downloading.lock().unwrap() {
            ctx.request_repaint();
        }
    }
}

pub fn run_gui() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([600.0, 400.0])
            .with_min_inner_size([400.0, 300.0])
            .with_resizable(true),
        ..Default::default()
    };

    eframe::run_native(
        "Hotmart Video Downloader",
        options,
        Box::new(|_cc| Ok(Box::new(HotmartGui::default()) as Box<dyn App>)),
    )
}
