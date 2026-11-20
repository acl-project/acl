// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use acl_core::task::ProgressUnit;
use acl_core::task::StageSnapshot;
use acl_core::task::StageStatus;
use libadwaita::prelude::*;
use relm4::SimpleComponent;
use relm4::prelude::*;

fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;

    let b = bytes as f64;
    if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

#[derive(Debug, Clone)]
pub struct StageRow {
    stage: StageSnapshot,
}

impl StageRow {
    fn status_icon(&self) -> &'static str {
        match self.stage.status {
            StageStatus::Pending | StageStatus::Running => "content-loading-symbolic",
            StageStatus::Done => "object-select-symbolic",
            StageStatus::Cancelled => "process-stop-symbolic",
            StageStatus::Failed => "dialog-error-symbolic",
        }
    }

    fn status_css(&self) -> &'static [&'static str] {
        match self.stage.status {
            StageStatus::Pending | StageStatus::Running | StageStatus::Cancelled => &["dim-label"],
            StageStatus::Done => &["success"],
            StageStatus::Failed => &["error"],
        }
    }

    fn fraction(&self) -> f64 {
        if self.stage.total == 0 {
            0.0
        } else {
            (self.stage.done as f64 / self.stage.total as f64).clamp(0.0, 1.0)
        }
    }
    fn progress_label(&self) -> String {
        match self.stage.unit {
            ProgressUnit::Count => format!("{} / {}", self.stage.done, self.stage.total),
            ProgressUnit::Bytes => format!(
                "{} / {}",
                format_bytes(self.stage.done),
                format_bytes(self.stage.total)
            ),
        }
    }
}

#[relm4::component(pub)]
impl SimpleComponent for StageRow {
    type Init = StageSnapshot;
    type Input = StageSnapshot;
    type Output = ();

    view! {
        #[root]
        adw::ActionRow {
            #[watch]
            set_title: &model.stage.label,

            add_prefix = &gtk::Box {
                set_valign: gtk::Align::Center,
                set_width_request: 16,

                adw::Spinner {
                    #[watch]
                    set_visible: model.stage.status == StageStatus::Running,
                },

                gtk::Image {
                    #[watch]
                    set_visible: model.stage.status != StageStatus::Running,
                    #[watch]
                    set_icon_name: Some(model.status_icon()),
                    #[watch]
                    set_css_classes: model.status_css(),
                }

            },

            add_suffix = &gtk::Label {
                set_valign: gtk::Align::Center,
                add_css_class: "numeric",
                add_css_class: "dim-label",
                #[watch]
                set_visible: model.stage.total > 1,
                #[watch]
                set_label: &model.progress_label(),
            },


            add_suffix = &gtk::ProgressBar {
                set_valign: gtk::Align::Center,
                set_width_request: 120,
                #[watch]
                set_visible: model.stage.total > 1,
                #[watch]
                set_fraction: model.fraction(),
            }
        }
    }

    fn init(
        stage: Self::Init,
        root: Self::Root,
        _sender: relm4::prelude::ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let model = Self { stage };
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, _sender: ComponentSender<Self>) {
        self.stage = msg;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row_with(status: StageStatus, done: u64, total: u64) -> StageRow {
        StageRow {
            stage: StageSnapshot {
                label: "Test stage".to_string(),
                status,
                done,
                total,
                bytes_per_sec: None,
                unit: acl_core::task::ProgressUnit::Count,
            },
        }
    }

    #[test]
    fn test_stage_row_status_icon_and_css() {
        assert_eq!(
            row_with(StageStatus::Pending, 0, 0).status_icon(),
            "content-loading-symbolic"
        );
        assert_eq!(
            row_with(StageStatus::Running, 0, 0).status_icon(),
            "content-loading-symbolic"
        );
        assert_eq!(
            row_with(StageStatus::Done, 0, 0).status_icon(),
            "object-select-symbolic"
        );
        assert_eq!(
            row_with(StageStatus::Cancelled, 0, 0).status_icon(),
            "process-stop-symbolic"
        );
        assert_eq!(
            row_with(StageStatus::Failed, 0, 0).status_icon(),
            "dialog-error-symbolic"
        );

        assert_eq!(
            row_with(StageStatus::Pending, 0, 0).status_css(),
            &["dim-label"]
        );
        assert_eq!(
            row_with(StageStatus::Running, 0, 0).status_css(),
            &["dim-label"]
        );
        assert_eq!(
            row_with(StageStatus::Cancelled, 0, 0).status_css(),
            &["dim-label"]
        );
        assert_eq!(row_with(StageStatus::Done, 0, 0).status_css(), &["success"]);
        assert_eq!(row_with(StageStatus::Failed, 0, 0).status_css(), &["error"]);
    }

    #[test]
    fn test_stage_row_fraction() {
        assert_eq!(row_with(StageStatus::Pending, 0, 0).fraction(), 0.0);
        assert_eq!(row_with(StageStatus::Pending, 5, 0).fraction(), 0.0);
        assert!((row_with(StageStatus::Running, 3, 10).fraction() - 0.3).abs() < f64::EPSILON);
        assert_eq!(row_with(StageStatus::Running, 15, 10).fraction(), 1.0);
    }

    #[test]
    fn test_format_bytes_and_progress_label() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1023), "1023 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(1024 * 1024), "1.0 MB");
        assert_eq!(format_bytes(1_572_864), "1.5 MB");

        assert_eq!(
            row_with(StageStatus::Running, 3, 10).progress_label(),
            "3 / 10"
        );

        let bytes_row = StageRow {
            stage: StageSnapshot {
                label: "Downloading".to_string(),
                status: StageStatus::Running,
                done: 1024,
                total: 1024 * 1024,
                bytes_per_sec: None,
                unit: ProgressUnit::Bytes,
            },
        };
        assert_eq!(bytes_row.progress_label(), "1.0 KB / 1.0 MB");
    }
}
