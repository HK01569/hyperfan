use libadwaita as adw;
use libadwaita::prelude::*;

pub struct SystemInfoCard {
    card: adw::PreferencesGroup,
}

impl SystemInfoCard {
    pub fn new() -> Self {
        let card = adw::PreferencesGroup::builder()
            .title("System Information")
            .build();

        match hf_core::get_system_summary() {
            Ok(info) => {
                Self::add_row(&card, "Hostname", &info.hostname);
                Self::add_row(&card, "Kernel", &info.kernel_version);
                Self::add_row(&card, "CPU", &info.cpu_model);
                Self::add_row(&card, "Cores", &info.cpu_cores.to_string());
                Self::add_row(&card, "Motherboard", &info.motherboard_name);
                Self::add_row(
                    &card,
                    "Memory",
                    &format!(
                        "{} MB available / {} MB total",
                        info.memory_available_mb, info.memory_total_mb
                    ),
                );
            }
            Err(e) => {
                let error_row = adw::ActionRow::builder()
                    .title("Error loading system info")
                    .subtitle(&e.to_string())
                    .build();
                card.add(&error_row);
            }
        }

        Self { card }
    }

    fn add_row(card: &adw::PreferencesGroup, title: &str, value: &str) {
        let row = adw::ActionRow::builder()
            .title(title)
            .subtitle(value)
            .build();
        card.add(&row);
    }

    pub fn widget(&self) -> &adw::PreferencesGroup {
        &self.card
    }
}
