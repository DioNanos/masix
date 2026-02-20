//! Masix Telegram Menu Handler with i18n
//!
//! Inline keyboard menus for interactive navigation

use masix_ipc::{InlineButton, OutboundMessage};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    English,
    Spanish,
    Chinese,
    Russian,
    Italian,
}

impl Default for Language {
    fn default() -> Self {
        Language::English
    }
}

impl std::str::FromStr for Language {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "en" | "english" => Ok(Language::English),
            "es" | "spanish" => Ok(Language::Spanish),
            "zh" | "chinese" => Ok(Language::Chinese),
            "ru" | "russian" => Ok(Language::Russian),
            "it" | "italian" => Ok(Language::Italian),
            _ => Err(format!("Unknown language: {}", s)),
        }
    }
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Language::English => write!(f, "en"),
            Language::Spanish => write!(f, "es"),
            Language::Chinese => write!(f, "zh"),
            Language::Russian => write!(f, "ru"),
            Language::Italian => write!(f, "it"),
        }
    }
}

pub fn home_menu(lang: Language) -> (String, Vec<Vec<InlineButton>>) {
    let title = match lang {
        Language::English => "🏠 *Masix Bot*\n\nSelect an option:",
        Language::Spanish => "🏠 *Masix Bot*\n\nSelecciona una opción:",
        Language::Chinese => "🏠 *Masix Bot*\n\n选择一个选项：",
        Language::Russian => "🏠 *Masix Bot*\n\nВыберите опцию:",
        Language::Italian => "🏠 *Masix Bot*\n\nSeleziona un'opzione:",
    };

    let chat = match lang {
        Language::English => "💬 Chat",
        Language::Spanish => "💬 Chat",
        Language::Chinese => "💬 聊天",
        Language::Russian => "💬 Чат",
        Language::Italian => "💬 Chat",
    };

    let reminder = match lang {
        Language::English => "⏰ Reminder",
        Language::Spanish => "⏰ Recordatorio",
        Language::Chinese => "⏰ 提醒",
        Language::Russian => "⏰ Напоминание",
        Language::Italian => "⏰ Promemoria",
    };

    let utility = match lang {
        Language::English => "🔧 Utility",
        Language::Spanish => "🔧 Utilidad",
        Language::Chinese => "🔧 工具",
        Language::Russian => "🔧 Утилиты",
        Language::Italian => "🔧 Utilità",
    };

    let settings = match lang {
        Language::English => "⚙️ Settings",
        Language::Spanish => "⚙️ Ajustes",
        Language::Chinese => "⚙️ 设置",
        Language::Russian => "⚙️ Настройки",
        Language::Italian => "⚙️ Impostazioni",
    };

    (
        title.to_string(),
        vec![
            vec![InlineButton {
                text: chat.to_string(),
                callback_data: "menu:chat".to_string(),
            }],
            vec![
                InlineButton {
                    text: reminder.to_string(),
                    callback_data: "menu:reminder".to_string(),
                },
                InlineButton {
                    text: utility.to_string(),
                    callback_data: "menu:utility".to_string(),
                },
            ],
            vec![InlineButton {
                text: settings.to_string(),
                callback_data: "menu:settings".to_string(),
            }],
        ],
    )
}

pub fn language_menu(lang: Language) -> (String, Vec<Vec<InlineButton>>) {
    let title = match lang {
        Language::English => "🌐 *Select Language*",
        Language::Spanish => "🌐 *Seleccionar Idioma*",
        Language::Chinese => "🌐 *选择语言*",
        Language::Russian => "🌐 *Выбрать язык*",
        Language::Italian => "🌐 *Seleziona Lingua*",
    };

    let back = match lang {
        Language::English => "⬅️ Back",
        Language::Spanish => "⬅️ Atrás",
        Language::Chinese => "⬅️ 返回",
        Language::Russian => "⬅️ Назад",
        Language::Italian => "⬅️ Indietro",
    };

    (
        title.to_string(),
        vec![
            vec![
                InlineButton {
                    text: "🇬🇧 English".to_string(),
                    callback_data: "lang:en".to_string(),
                },
                InlineButton {
                    text: "🇪🇸 Español".to_string(),
                    callback_data: "lang:es".to_string(),
                },
            ],
            vec![
                InlineButton {
                    text: "🇨🇳 中文".to_string(),
                    callback_data: "lang:zh".to_string(),
                },
                InlineButton {
                    text: "🇷🇺 Русский".to_string(),
                    callback_data: "lang:ru".to_string(),
                },
            ],
            vec![InlineButton {
                text: "🇮🇹 Italiano".to_string(),
                callback_data: "lang:it".to_string(),
            }],
            vec![InlineButton {
                text: back.to_string(),
                callback_data: "menu:settings".to_string(),
            }],
        ],
    )
}

pub fn help_text(lang: Language) -> String {
    match lang {
        Language::English => "📚 *Help - Available Commands*\n\n/start - Show main menu\n/menu - Show main menu\n/new - Reset conversation\n/help - Show this help\n/language - Change language\n\nJust send a message to chat with me!",
        Language::Spanish => "📚 *Ayuda - Comandos Disponibles*\n\n/start - Mostrar menú principal\n/menu - Mostrar menú principal\n/new - Reiniciar conversación\n/help - Mostrar esta ayuda\n/language - Cambiar idioma\n\n¡Solo envía un mensaje para chatear conmigo!",
        Language::Chinese => "📚 *帮助 - 可用命令*\n\n/start - 显示主菜单\n/menu - 显示主菜单\n/new - 重置对话\n/help - 显示帮助\n/language - 更改语言\n\n只需发送消息与我聊天！",
        Language::Russian => "📚 *Помощь - Доступные команды*\n\n/start - Показать главное меню\n/menu - Показать главное меню\n/new - Сбросить разговор\n/help - Показать помощь\n/language - Сменить язык\n\nПросто отправьте сообщение, чтобы пообщаться!",
        Language::Italian => "📚 *Aiuto - Comandi Disponibili*\n\n/start - Mostra menu principale\n/menu - Mostra menu principale\n/new - Resetta conversazione\n/help - Mostra aiuto\n/language - Cambia lingua\n\nInvia un messaggio per chiacchierare con me!",
    }.to_string()
}

pub fn command_list(lang: Language) -> String {
    match lang {
        Language::English => "📋 *Commands*\n\n/start - Main menu\n/menu - Main menu\n/new - Reset session\n/help - Help\n/language - Language\n/cron - Reminders\n/exec - Run commands\n/termux - Termux tools",
        Language::Spanish => "📋 *Comandos*\n\n/start - Menú principal\n/menu - Menú principal\n/new - Reiniciar sesión\n/help - Ayuda\n/language - Idioma\n/cron - Recordatorios\n/exec - Ejecutar comandos\n/termux - Herramientas Termux",
        Language::Chinese => "📋 *命令*\n\n/start - 主菜单\n/menu - 主菜单\n/new - 重置会话\n/help - 帮助\n/language - 语言\n/cron - 提醒\n/exec - 执行命令\n/termux - Termux工具",
        Language::Russian => "📋 *Команды*\n\n/start - Главное меню\n/menu - Главное меню\n/new - Сброс сессии\n/help - Помощь\n/language - Язык\n/cron - Напоминания\n/exec - Выполнить команды\n/termux - Инструменты Termux",
        Language::Italian => "📋 *Comandi*\n\n/start - Menu principale\n/menu - Menu principale\n/new - Reset sessione\n/help - Aiuto\n/language - Lingua\n/cron - Promemoria\n/exec - Esegui comandi\n/termux - Strumenti Termux",
    }.to_string()
}

pub fn session_reset_text(lang: Language) -> String {
    match lang {
        Language::English => "🔄 *Session Reset*\n\nConversation history cleared. Starting fresh!",
        Language::Spanish => {
            "🔄 *Sesión Reiniciada*\n\nHistorial de conversación borrado. ¡Empezando de nuevo!"
        }
        Language::Chinese => "🔄 *会话重置*\n\n对话历史已清除。重新开始！",
        Language::Russian => "🔄 *Сброс сессии*\n\nИстория очищена. Начинаем заново!",
        Language::Italian => {
            "🔄 *Sessione Reimpostata*\n\nCronologia conversazione cancellata. Ricomincio!"
        }
    }
    .to_string()
}

pub fn language_changed_text(new_lang: Language) -> String {
    match new_lang {
        Language::English => "✅ Language changed to English",
        Language::Spanish => "✅ Idioma cambiado a Español",
        Language::Chinese => "✅ 语言已更改为中文",
        Language::Russian => "✅ Язык изменён на Русский",
        Language::Italian => "✅ Lingua cambiata in Italiano",
    }
    .to_string()
}

pub fn nav_back(lang: Language) -> InlineButton {
    let text = match lang {
        Language::English => "⬅️ Back",
        Language::Spanish => "⬅️ Atrás",
        Language::Chinese => "⬅️ 返回",
        Language::Russian => "⬅️ Назад",
        Language::Italian => "⬅️ Indietro",
    };
    InlineButton {
        text: text.to_string(),
        callback_data: "menu:home".to_string(),
    }
}

// Keep backward compatibility
pub fn home_menu_legacy() -> (String, Vec<Vec<InlineButton>>) {
    home_menu(Language::English)
}

pub fn reminder_menu(lang: Language) -> (String, Vec<Vec<InlineButton>>) {
    let title = match lang {
        Language::English => "⏰ *Reminder*\n\nManage your reminders:",
        Language::Spanish => "⏰ *Recordatorio*\n\nGestiona tus recordatorios:",
        Language::Chinese => "⏰ *提醒*\n\n管理您的提醒：",
        Language::Russian => "⏰ *Напоминание*\n\nУправление напоминаниями:",
        Language::Italian => "⏰ *Promemoria*\n\nGestisci i tuoi promemoria:",
    };

    let add = match lang {
        Language::English => "➕ New",
        Language::Spanish => "➕ Nuevo",
        Language::Chinese => "➕ 新建",
        Language::Russian => "➕ Новый",
        Language::Italian => "➕ Nuovo",
    };

    let list = match lang {
        Language::English => "📋 List",
        Language::Spanish => "📋 Lista",
        Language::Chinese => "📋 列表",
        Language::Russian => "📋 Список",
        Language::Italian => "📋 Lista",
    };

    let home = match lang {
        Language::English => "🏠 Home",
        Language::Spanish => "🏠 Inicio",
        Language::Chinese => "🏠 首页",
        Language::Russian => "🏠 Главная",
        Language::Italian => "🏠 Home",
    };

    (
        title.to_string(),
        vec![
            vec![
                InlineButton {
                    text: add.to_string(),
                    callback_data: "reminder:add".to_string(),
                },
                InlineButton {
                    text: list.to_string(),
                    callback_data: "reminder:list".to_string(),
                },
            ],
            vec![InlineButton {
                text: home.to_string(),
                callback_data: "menu:home".to_string(),
            }],
        ],
    )
}

pub fn utility_menu(lang: Language) -> (String, Vec<Vec<InlineButton>>) {
    let title = match lang {
        Language::English => "🔧 *Utility*\n\nAvailable tools:",
        Language::Spanish => "🔧 *Utilidad*\n\nHerramientas disponibles:",
        Language::Chinese => "🔧 *工具*\n\n可用工具：",
        Language::Russian => "🔧 *Утилиты*\n\nДоступные инструменты:",
        Language::Italian => "🔧 *Utilità*\n\nStrumenti disponibili:",
    };

    (
        title.to_string(),
        vec![
            vec![InlineButton {
                text: "📁 Filesystem".to_string(),
                callback_data: "utility:fs".to_string(),
            }],
            vec![InlineButton {
                text: "🖥️ Exec".to_string(),
                callback_data: "utility:exec".to_string(),
            }],
            vec![InlineButton {
                text: "📱 Termux".to_string(),
                callback_data: "utility:termux".to_string(),
            }],
            vec![nav_back(lang)],
        ],
    )
}

pub fn settings_menu(lang: Language) -> (String, Vec<Vec<InlineButton>>) {
    let title = match lang {
        Language::English => "⚙️ *Settings*\n\nBot configuration:",
        Language::Spanish => "⚙️ *Ajustes*\n\nConfiguración del bot:",
        Language::Chinese => "⚙️ *设置*\n\n机器人配置：",
        Language::Russian => "⚙️ *Настройки*\n\nКонфигурация бота:",
        Language::Italian => "⚙️ *Impostazioni*\n\nConfigurazione bot:",
    };

    let language = match lang {
        Language::English => "🌐 Language",
        Language::Spanish => "🌐 Idioma",
        Language::Chinese => "🌐 语言",
        Language::Russian => "🌐 Язык",
        Language::Italian => "🌐 Lingua",
    };

    let stats = match lang {
        Language::English => "📈 Statistics",
        Language::Spanish => "📈 Estadísticas",
        Language::Chinese => "📈 统计",
        Language::Russian => "📈 Статистика",
        Language::Italian => "📈 Statistiche",
    };

    let home = match lang {
        Language::English => "🏠 Home",
        Language::Spanish => "🏠 Inicio",
        Language::Chinese => "🏠 首页",
        Language::Russian => "🏠 Главная",
        Language::Italian => "🏠 Home",
    };

    (
        title.to_string(),
        vec![
            vec![InlineButton {
                text: language.to_string(),
                callback_data: "menu:language".to_string(),
            }],
            vec![InlineButton {
                text: stats.to_string(),
                callback_data: "settings:stats".to_string(),
            }],
            vec![InlineButton {
                text: home.to_string(),
                callback_data: "menu:home".to_string(),
            }],
        ],
    )
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
        chat_action: None,
    }
}

pub fn handle_callback(
    data: &str,
    chat_id: i64,
    message_id: Option<i64>,
    account_tag: Option<String>,
    lang: Language,
) -> Option<OutboundMessage> {
    let parts: Vec<&str> = data.split(':').collect();
    if parts.is_empty() {
        return None;
    }

    match parts[0] {
        "menu" => {
            let (text, keyboard) = match parts.get(1).map(|s| *s) {
                Some("home") => home_menu(lang),
                Some("reminder") => reminder_menu(lang),
                Some("utility") => utility_menu(lang),
                Some("settings") => settings_menu(lang),
                Some("language") => language_menu(lang),
                Some("chat") => {
                    let msg = match lang {
                        Language::English => "💬 *Chat Mode*\n\nSend me a message to chat!",
                        Language::Spanish => "💬 *Modo Chat*\n\n¡Envíame un mensaje para chatear!",
                        Language::Chinese => "💬 *聊天模式*\n\n发消息给我聊天！",
                        Language::Russian => "💬 *Режим чата*\n\nОтправьте сообщение для общения!",
                        Language::Italian => {
                            "💬 *Modalità Chat*\n\nInviami un messaggio per chiacchierare!"
                        }
                    };
                    return Some(action_message(
                        chat_id,
                        account_tag,
                        message_id,
                        msg.to_string(),
                        Some(vec![vec![nav_back(lang)]]),
                    ));
                }
                _ => return None,
            };
            Some(action_message(
                chat_id,
                account_tag,
                message_id,
                text,
                Some(keyboard),
            ))
        }
        "lang" => {
            if let Some(code) = parts.get(1) {
                if let Ok(new_lang) = code.parse::<Language>() {
                    let text = language_changed_text(new_lang);
                    let (_, keyboard) = settings_menu(new_lang);
                    return Some(action_message(
                        chat_id,
                        account_tag,
                        message_id,
                        text,
                        Some(keyboard),
                    ));
                }
            }
            None
        }
        "reminder" => {
            let msg = match parts.get(1).map(|s| *s) {
                Some("add") => match lang {
                    Language::English => "➕ *New Reminder*\n\nUse: `/cron Tomorrow at 9am reminder \"Meeting\"`",
                    Language::Spanish => "➕ *Nuevo Recordatorio*\n\nUsa: `/cron Mañana a las 9am reminder \"Reunión\"`",
                    Language::Chinese => "➕ *新提醒*\n\n使用: `/cron 明天上午9点 reminder \"会议\"`",
                    Language::Russian => "➕ *Новое напоминание*\n\nИспользуйте: `/cron Завтра в 9 утра reminder \"Встреча\"`",
                    Language::Italian => "➕ *Nuovo Promemoria*\n\nUsa: `/cron Domani alle 9 promemoria \"Meeting\"`",
                },
                Some("list") => match lang {
                    Language::English => "📋 Use `/cron list` to see your reminders.",
                    Language::Spanish => "📋 Usa `/cron list` para ver tus recordatorios.",
                    Language::Chinese => "📋 使用 `/cron list` 查看您的提醒。",
                    Language::Russian => "📋 Используйте `/cron list` для просмотра напоминаний.",
                    Language::Italian => "📋 Usa `/cron list` per vedere i promemoria.",
                },
                _ => return None,
            };
            let (_, menu_keyboard) = reminder_menu(lang);
            Some(action_message(
                chat_id,
                account_tag,
                message_id,
                msg.to_string(),
                Some(menu_keyboard),
            ))
        }
        "utility" => {
            let msg = match parts.get(1).map(|s| *s) {
                Some("fs") => match lang {
                    Language::English => "📁 *Filesystem*\n\nAsk me to read or write files.",
                    Language::Spanish => "📁 *Archivos*\n\nPídeme leer o escribir archivos.",
                    Language::Chinese => "📁 *文件系统*\n\n让我读写文件。",
                    Language::Russian => "📁 *Файлы*\n\nПопросите меня читать или писать файлы.",
                    Language::Italian => "📁 *File*\n\nChiedimi di leggere o scrivere file.",
                },
                Some("exec") => match lang {
                    Language::English => "🖥️ *Exec*\n\nRun commands: `/exec ls -la`",
                    Language::Spanish => "🖥️ *Ejecutar*\n\nEjecuta comandos: `/exec ls -la`",
                    Language::Chinese => "🖥️ *执行*\n\n运行命令: `/exec ls -la`",
                    Language::Russian => "🖥️ *Выполнить*\n\nЗапустите команды: `/exec ls -la`",
                    Language::Italian => "🖥️ *Esegui*\n\nEsegui comandi: `/exec ls -la`",
                },
                Some("termux") => match lang {
                    Language::English => {
                        "📱 *Termux*\n\nCommands: `/termux battery`, `/termux info`"
                    }
                    Language::Spanish => {
                        "📱 *Termux*\n\nComandos: `/termux battery`, `/termux info`"
                    }
                    Language::Chinese => "📱 *Termux*\n\n命令: `/termux battery`, `/termux info`",
                    Language::Russian => {
                        "📱 *Termux*\n\nКоманды: `/termux battery`, `/termux info`"
                    }
                    Language::Italian => {
                        "📱 *Termux*\n\nComandi: `/termux battery`, `/termux info`"
                    }
                },
                _ => return None,
            };
            let (_, menu_keyboard) = utility_menu(lang);
            Some(action_message(
                chat_id,
                account_tag,
                message_id,
                msg.to_string(),
                Some(menu_keyboard),
            ))
        }
        "settings" => {
            let msg = match parts.get(1).map(|s| *s) {
                Some("stats") => match lang {
                    Language::English => "📈 *Statistics*\n\nComing soon!",
                    Language::Spanish => "📈 *Estadísticas*\n\n¡Próximamente!",
                    Language::Chinese => "📈 *统计*\n\n即将推出！",
                    Language::Russian => "📈 *Статистика*\n\nСкоро!",
                    Language::Italian => "📈 *Statistiche*\n\nArriveranno presto!",
                },
                _ => return None,
            };
            let (_, menu_keyboard) = settings_menu(lang);
            Some(action_message(
                chat_id,
                account_tag,
                message_id,
                msg.to_string(),
                Some(menu_keyboard),
            ))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_home_menu() {
        let (text, keyboard) = home_menu(Language::English);
        assert!(text.contains("Masix Bot"));
        assert!(!keyboard.is_empty());
    }

    #[test]
    fn test_language_parse() {
        assert_eq!("en".parse::<Language>().unwrap(), Language::English);
        assert_eq!("it".parse::<Language>().unwrap(), Language::Italian);
    }
}
