//! Masix Telegram Menu Handler
//!
//! Inline keyboard menus for interactive navigation

use masix_ipc::{InlineButton, OutboundMessage};

pub fn home_menu() -> (String, Vec<Vec<InlineButton>>) {
    (
        "🏠 *Masix Bot*\n\nSeleziona un'opzione:".to_string(),
        vec![
            vec![
                InlineButton {
                    text: "⏰ Reminder".to_string(),
                    callback_data: "menu:reminder".to_string(),
                },
                InlineButton {
                    text: "🔧 Utility".to_string(),
                    callback_data: "menu:utility".to_string(),
                },
            ],
            vec![InlineButton {
                text: "⚙️ Settings".to_string(),
                callback_data: "menu:settings".to_string(),
            }],
        ],
    )
}

pub fn reminder_menu() -> (String, Vec<Vec<InlineButton>>) {
    (
        "⏰ *Reminder*\n\nGestione promemoria:".to_string(),
        vec![
            vec![
                InlineButton {
                    text: "➕ Nuovo".to_string(),
                    callback_data: "reminder:add".to_string(),
                },
                InlineButton {
                    text: "📋 Lista".to_string(),
                    callback_data: "reminder:list".to_string(),
                },
            ],
            vec![InlineButton {
                text: "🏠 Home".to_string(),
                callback_data: "menu:home".to_string(),
            }],
        ],
    )
}

pub fn utility_menu() -> (String, Vec<Vec<InlineButton>>) {
    (
        "🔧 *Utility*\n\nStrumenti disponibili:".to_string(),
        vec![
            vec![
                InlineButton {
                    text: "📁 Filesystem".to_string(),
                    callback_data: "utility:fs".to_string(),
                },
                InlineButton {
                    text: "🖥 Exec".to_string(),
                    callback_data: "utility:exec".to_string(),
                },
            ],
            vec![
                InlineButton {
                    text: "📱 Termux".to_string(),
                    callback_data: "utility:termux".to_string(),
                },
                InlineButton {
                    text: "📊 Info Sistema".to_string(),
                    callback_data: "utility:info".to_string(),
                },
            ],
            vec![InlineButton {
                text: "🏠 Home".to_string(),
                callback_data: "menu:home".to_string(),
            }],
        ],
    )
}

pub fn settings_menu() -> (String, Vec<Vec<InlineButton>>) {
    (
        "⚙️ *Impostazioni*\n\nConfigurazione bot:".to_string(),
        vec![
            vec![InlineButton {
                text: "🔄 Ricarica Config".to_string(),
                callback_data: "settings:reload".to_string(),
            }],
            vec![InlineButton {
                text: "📈 Statistiche".to_string(),
                callback_data: "settings:stats".to_string(),
            }],
            vec![InlineButton {
                text: "🏠 Home".to_string(),
                callback_data: "menu:home".to_string(),
            }],
        ],
    )
}

fn nav_keyboard(back_callback: &str) -> Vec<Vec<InlineButton>> {
    vec![
        vec![InlineButton {
            text: "⬅️ Indietro".to_string(),
            callback_data: back_callback.to_string(),
        }],
        vec![InlineButton {
            text: "🏠 Home".to_string(),
            callback_data: "menu:home".to_string(),
        }],
    ]
}

fn action_message(
    chat_id: i64,
    account_tag: Option<String>,
    message_id: Option<i64>,
    text: String,
    keyboard: Option<Vec<Vec<InlineButton>>>,
) -> OutboundMessage {
    OutboundMessage {
        channel: "telegram".to_string(),
        account_tag,
        chat_id,
        text,
        reply_to: None,
        edit_message_id: message_id,
        inline_keyboard: keyboard,
    }
}

pub fn handle_callback(
    data: &str,
    chat_id: i64,
    message_id: Option<i64>,
    account_tag: Option<String>,
) -> Option<OutboundMessage> {
    let parts: Vec<&str> = data.split(':').collect();
    if parts.len() < 2 {
        return None;
    }

    let (text, keyboard) = match parts[0] {
        "menu" => match parts[1] {
            "home" => home_menu(),
            "reminder" => reminder_menu(),
            "utility" => utility_menu(),
            "settings" => settings_menu(),
            _ => return None,
        },
        "reminder" => match parts[1] {
            "add" => {
                return Some(action_message(
                    chat_id,
                    account_tag.clone(),
                    message_id,
                    "Scrivi il reminder, ad esempio:\n`/cron Domani alle 9 promemoria \"Meeting\"`"
                        .to_string(),
                    Some(vec![
                        vec![InlineButton {
                            text: "📋 Lista".to_string(),
                            callback_data: "reminder:list".to_string(),
                        }],
                        vec![InlineButton {
                            text: "⬅️ Reminder".to_string(),
                            callback_data: "menu:reminder".to_string(),
                        }],
                        vec![InlineButton {
                            text: "🏠 Home".to_string(),
                            callback_data: "menu:home".to_string(),
                        }],
                    ]),
                ));
            }
            "list" => {
                return Some(action_message(
                    chat_id,
                    account_tag.clone(),
                    message_id,
                    "Usa `/cron list` per vedere i reminder attivi.".to_string(),
                    Some(vec![
                        vec![InlineButton {
                            text: "➕ Nuovo".to_string(),
                            callback_data: "reminder:add".to_string(),
                        }],
                        vec![InlineButton {
                            text: "⬅️ Reminder".to_string(),
                            callback_data: "menu:reminder".to_string(),
                        }],
                        vec![InlineButton {
                            text: "🏠 Home".to_string(),
                            callback_data: "menu:home".to_string(),
                        }],
                    ]),
                ));
            }
            _ => return None,
        },
        "utility" => match parts[1] {
            "fs" => {
                return Some(action_message(
                    chat_id,
                    account_tag.clone(),
                    message_id,
                    "Chiedimi di leggere o scrivere file.\nEsempio: \"Leggi il file /home/user/test.txt\""
                        .to_string(),
                    Some(nav_keyboard("menu:utility")),
                ));
            }
            "exec" => {
                return Some(action_message(
                    chat_id,
                    account_tag.clone(),
                    message_id,
                    "Esecuzione comandi base (allowlist) nella workdir del bot.\n\
                     Esempi:\n\
                     - `/exec pwd`\n\
                     - `/exec ls -la`\n\
                     - `/exec df -h`"
                        .to_string(),
                    Some(nav_keyboard("menu:utility")),
                ));
            }
            "termux" => {
                return Some(action_message(
                    chat_id,
                    account_tag.clone(),
                    message_id,
                    "Comandi Termux disponibili.\n\
                     Esempi:\n\
                     - `/termux battery`\n\
                     - `/termux info`\n\
                     - `/termux cmd termux-location`\n\
                     - `/termux boot status`"
                        .to_string(),
                    Some(vec![
                        vec![
                            InlineButton {
                                text: "🔋 Battery".to_string(),
                                callback_data: "utility:termux_battery".to_string(),
                            },
                            InlineButton {
                                text: "ℹ️ Info".to_string(),
                                callback_data: "utility:termux_info".to_string(),
                            },
                        ],
                        vec![InlineButton {
                            text: "🚀 Boot".to_string(),
                            callback_data: "utility:termux_boot".to_string(),
                        }],
                        vec![InlineButton {
                            text: "⬅️ Utility".to_string(),
                            callback_data: "menu:utility".to_string(),
                        }],
                        vec![InlineButton {
                            text: "🏠 Home".to_string(),
                            callback_data: "menu:home".to_string(),
                        }],
                    ]),
                ));
            }
            "termux_battery" => {
                return Some(action_message(
                    chat_id,
                    account_tag.clone(),
                    message_id,
                    "Esegui: `/termux battery`".to_string(),
                    Some(nav_keyboard("utility:termux")),
                ));
            }
            "termux_info" => {
                return Some(action_message(
                    chat_id,
                    account_tag.clone(),
                    message_id,
                    "Esegui: `/termux info`".to_string(),
                    Some(nav_keyboard("utility:termux")),
                ));
            }
            "termux_boot" => {
                return Some(action_message(
                    chat_id,
                    account_tag.clone(),
                    message_id,
                    "Gestione avvio automatico su boot Android:\n\
                     - `/termux boot on`\n\
                     - `/termux boot off`\n\
                     - `/termux boot status`\n\
                     Richiede app Termux:Boot installata."
                        .to_string(),
                    Some(nav_keyboard("utility:termux")),
                ));
            }
            "info" => {
                return Some(action_message(
                    chat_id,
                    account_tag.clone(),
                    message_id,
                    "ℹ️ *Masix Bot*\nVersione: 0.1.0\nRuntime: Rust/Tokio\nStorage: SQLite"
                        .to_string(),
                    Some(nav_keyboard("menu:utility")),
                ));
            }
            _ => return None,
        },
        "settings" => match parts[1] {
            "reload" => {
                return Some(action_message(
                    chat_id,
                    account_tag.clone(),
                    message_id,
                    "⚠️ Ricaricamento config non ancora implementato.".to_string(),
                    Some(nav_keyboard("menu:settings")),
                ));
            }
            "stats" => {
                return Some(action_message(
                    chat_id,
                    account_tag.clone(),
                    message_id,
                    "📊 Statistiche non ancora implementate.".to_string(),
                    Some(nav_keyboard("menu:settings")),
                ));
            }
            _ => return None,
        },
        _ => return None,
    };

    Some(OutboundMessage {
        channel: "telegram".to_string(),
        account_tag,
        chat_id,
        text,
        reply_to: None,
        edit_message_id: message_id,
        inline_keyboard: Some(keyboard),
    })
}

#[cfg(test)]
mod tests {
    use super::handle_callback;

    #[test]
    fn reminder_add_callback_returns_editable_interactive_message() {
        let out = handle_callback("reminder:add", 123, Some(77), Some("bot".to_string()))
            .expect("expected outbound");
        assert_eq!(out.edit_message_id, Some(77));
        assert!(out.inline_keyboard.is_some());
        assert!(out.text.contains("/cron"));
    }

    #[test]
    fn unknown_callback_returns_none() {
        assert!(handle_callback("unknown:test", 1, Some(1), None).is_none());
    }

    #[test]
    fn utility_termux_callback_returns_buttons() {
        let out = handle_callback("utility:termux", 123, Some(77), Some("bot".to_string()))
            .expect("expected outbound");
        assert!(out.inline_keyboard.is_some());
        assert!(out.text.contains("/termux"));
    }
}
