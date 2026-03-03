//! Masix Telegram Menu Handler with i18n
//!
//! Inline keyboard menus for interactive navigation

use masix_ipc::{InlineButton, OutboundMessage};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Language {
    #[default]
    English,
    Spanish,
    Chinese,
    Russian,
    Italian,
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

pub fn home_menu(lang: Language, is_admin: bool) -> (String, Vec<Vec<InlineButton>>) {
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

    let admin = match lang {
        Language::English => "🛡️ Admin",
        Language::Spanish => "🛡️ Admin",
        Language::Chinese => "🛡️ 管理",
        Language::Russian => "🛡️ Админ",
        Language::Italian => "🛡️ Admin",
    };

    let mut keyboard = vec![
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
    ];

    if is_admin {
        keyboard.push(vec![InlineButton {
            text: admin.to_string(),
            callback_data: "menu:admin".to_string(),
        }]);
    }

    keyboard.push(vec![InlineButton {
        text: settings.to_string(),
        callback_data: "menu:settings".to_string(),
    }]);

    (title.to_string(), keyboard)
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

pub fn help_text(lang: Language, is_admin: bool) -> String {
    let mut text = match lang {
        Language::English => "📚 *Help - Available Commands*\n\n/start - Show main menu\n/menu - Show main menu\n/new - Reset conversation\n/help - Show this help\n/whoiam - Show user/chat IDs\n/language - Change language\n/provider - Manage LLM provider\n/model - Change model\n/cron - Manage reminders\n/termux - Termux tools\n\nJust send a message to chat with me!",
        Language::Spanish => "📚 *Ayuda - Comandos Disponibles*\n\n/start - Mostrar menú principal\n/menu - Mostrar menú principal\n/new - Reiniciar conversación\n/help - Mostrar esta ayuda\n/whoiam - Mostrar IDs de usuario/chat\n/language - Cambiar idioma\n/provider - Gestionar proveedor LLM\n/model - Cambiar modelo\n/cron - Gestionar recordatorios\n/termux - Herramientas Termux\n\n¡Solo envía un mensaje para chatear conmigo!",
        Language::Chinese => "📚 *帮助 - 可用命令*\n\n/start - 显示主菜单\n/menu - 显示主菜单\n/new - 重置对话\n/help - 显示帮助\n/whoiam - 查看用户/聊天ID\n/language - 更改语言\n/provider - 管理LLM提供商\n/model - 更改模型\n/cron - 管理提醒\n/termux - Termux工具\n\n只需发送消息与我聊天！",
        Language::Russian => "📚 *Помощь - Доступные команды*\n\n/start - Показать главное меню\n/menu - Показать главное меню\n/new - Сбросить разговор\n/help - Показать помощь\n/whoiam - Показать ID пользователя/чата\n/language - Сменить язык\n/provider - Управление провайдером\n/model - Изменить модель\n/cron - Напоминания\n/termux - Инструменты Termux\n\nПросто отправьте сообщение, чтобы пообщаться!",
        Language::Italian => "📚 *Aiuto - Comandi Disponibili*\n\n/start - Mostra menu principale\n/menu - Mostra menu principale\n/new - Resetta conversazione\n/help - Mostra aiuto\n/whoiam - Mostra ID utente/chat\n/language - Cambia lingua\n/provider - Gestisci provider LLM\n/model - Cambia modello\n/cron - Gestisci promemoria\n/termux - Strumenti Termux\n\nInvia un messaggio per chiacchierare con me!",
    }
    .to_string();

    if is_admin {
        let admin_block = match lang {
            Language::English => "\n\n🛡️ *Admin commands*\n/admin - ACL and user tools\n/plugin - Module keys and catalog\n/mcp - MCP status\n/tools - Runtime tools list\n/exec - Run allowlisted shell command",
            Language::Spanish => "\n\n🛡️ *Comandos admin*\n/admin - ACL y tools de usuario\n/plugin - Claves de módulos y catálogo\n/mcp - Estado MCP\n/tools - Lista tools runtime\n/exec - Ejecutar comando allowlist",
            Language::Chinese => "\n\n🛡️ *管理员命令*\n/admin - ACL与用户工具\n/plugin - 模块密钥与目录\n/mcp - MCP状态\n/tools - 运行时工具列表\n/exec - 执行白名单命令",
            Language::Russian => "\n\n🛡️ *Команды администратора*\n/admin - ACL и инструменты пользователей\n/plugin - Ключи модулей и каталог\n/mcp - Статус MCP\n/tools - Список инструментов runtime\n/exec - Выполнить команду из allowlist",
            Language::Italian => "\n\n🛡️ *Comandi admin*\n/admin - ACL e tool utenti\n/plugin - Chiavi moduli e catalogo\n/mcp - Stato MCP\n/tools - Lista tool runtime\n/exec - Esegui comando allowlist",
        };
        text.push_str(admin_block);
    }

    text
}

pub fn command_list(lang: Language, is_admin: bool) -> String {
    let mut text = match lang {
        Language::English => "📋 *Commands*\n\n/start - Main menu\n/menu - Main menu\n/new - Reset session\n/help - Help\n/whoiam - Show user/chat IDs\n/language - Language\n/provider - LLM provider\n/model - Change model\n/cron - Reminders\n/termux - Termux tools",
        Language::Spanish => "📋 *Comandos*\n\n/start - Menú principal\n/menu - Menú principal\n/new - Reiniciar sesión\n/help - Ayuda\n/whoiam - Mostrar IDs usuario/chat\n/language - Idioma\n/provider - Proveedor LLM\n/model - Cambiar modelo\n/cron - Recordatorios\n/termux - Herramientas Termux",
        Language::Chinese => "📋 *命令*\n\n/start - 主菜单\n/menu - 主菜单\n/new - 重置会话\n/help - 帮助\n/whoiam - 查看用户/聊天ID\n/language - 语言\n/provider - LLM提供商\n/model - 更改模型\n/cron - 提醒\n/termux - Termux工具",
        Language::Russian => "📋 *Команды*\n\n/start - Главное меню\n/menu - Главное меню\n/new - Сброс сессии\n/help - Помощь\n/whoiam - Показать ID пользователя/чата\n/language - Язык\n/provider - Провайдер LLM\n/model - Изменить модель\n/cron - Напоминания\n/termux - Инструменты Termux",
        Language::Italian => "📋 *Comandi*\n\n/start - Menu principale\n/menu - Menu principale\n/new - Reset sessione\n/help - Aiuto\n/whoiam - Mostra ID utente/chat\n/language - Lingua\n/provider - Provider LLM\n/model - Cambia modello\n/cron - Promemoria\n/termux - Strumenti Termux",
    }
    .to_string();

    if is_admin {
        let admin_block = match lang {
            Language::English => "\n/admin - ACL and user tools\n/plugin - Module keys and catalog\n/mcp - MCP status\n/tools - Runtime tools list\n/exec - Run commands",
            Language::Spanish => "\n/admin - ACL y tools de usuario\n/plugin - Claves de módulos y catálogo\n/mcp - Estado MCP\n/tools - Lista tools runtime\n/exec - Ejecutar comandos",
            Language::Chinese => "\n/admin - ACL与用户工具\n/plugin - 模块密钥与目录\n/mcp - MCP状态\n/tools - 运行时工具列表\n/exec - 执行命令",
            Language::Russian => "\n/admin - ACL и инструменты пользователей\n/plugin - Ключи модулей и каталог\n/mcp - Статус MCP\n/tools - Список инструментов runtime\n/exec - Выполнить команды",
            Language::Italian => "\n/admin - ACL e tool utenti\n/plugin - Chiavi moduli e catalogo\n/mcp - Stato MCP\n/tools - Lista tool runtime\n/exec - Esegui comandi",
        };
        text.push_str(admin_block);
    }

    text
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
    home_menu(Language::English, false)
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

pub fn utility_menu(lang: Language, is_admin: bool) -> (String, Vec<Vec<InlineButton>>) {
    let title = match lang {
        Language::English => "🔧 *Utility*\n\nAvailable tools:",
        Language::Spanish => "🔧 *Utilidad*\n\nHerramientas disponibles:",
        Language::Chinese => "🔧 *工具*\n\n可用工具：",
        Language::Russian => "🔧 *Утилиты*\n\nДоступные инструменты:",
        Language::Italian => "🔧 *Utilità*\n\nStrumenti disponibili:",
    };

    let mut keyboard = vec![vec![InlineButton {
        text: "📁 Filesystem".to_string(),
        callback_data: "utility:fs".to_string(),
    }]];

    if is_admin {
        keyboard.push(vec![InlineButton {
            text: "🖥️ Exec".to_string(),
            callback_data: "utility:exec".to_string(),
        }]);
    }

    keyboard.push(vec![InlineButton {
        text: "📱 Termux".to_string(),
        callback_data: "utility:termux".to_string(),
    }]);
    keyboard.push(vec![nav_back(lang)]);

    (title.to_string(), keyboard)
}

pub fn admin_menu(lang: Language) -> (String, Vec<Vec<InlineButton>>) {
    let title = match lang {
        Language::English => "🛡️ *Admin*\n\nAdmin-only controls:",
        Language::Spanish => "🛡️ *Admin*\n\nControles solo admin:",
        Language::Chinese => "🛡️ *管理员*\n\n仅管理员控制：",
        Language::Russian => "🛡️ *Админ*\n\nУправление только для админа:",
        Language::Italian => "🛡️ *Admin*\n\nControlli solo admin:",
    };

    (
        title.to_string(),
        vec![
            vec![
                InlineButton {
                    text: "👥 ACL".to_string(),
                    callback_data: "admin:acl".to_string(),
                },
                InlineButton {
                    text: "🧰 User Tools".to_string(),
                    callback_data: "admin:user_tools".to_string(),
                },
            ],
            vec![
                InlineButton {
                    text: "🔌 Runtime".to_string(),
                    callback_data: "admin:runtime".to_string(),
                },
                InlineButton {
                    text: "🖥️ Exec".to_string(),
                    callback_data: "admin:exec".to_string(),
                },
            ],
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

    let help = match lang {
        Language::English => "❓ Help",
        Language::Spanish => "❓ Ayuda",
        Language::Chinese => "❓ 帮助",
        Language::Russian => "❓ Помощь",
        Language::Italian => "❓ Aiuto",
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
                text: help.to_string(),
                callback_data: "settings:help".to_string(),
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
        draft_id: None,
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
    is_admin: bool,
) -> Option<OutboundMessage> {
    let parts: Vec<&str> = data.split(':').collect();
    if parts.is_empty() {
        return None;
    }

    match parts[0] {
        "menu" => {
            let (text, keyboard) = match parts.get(1).copied() {
                Some("home") => home_menu(lang, is_admin),
                Some("reminder") => reminder_menu(lang),
                Some("utility") => utility_menu(lang, is_admin),
                Some("settings") => settings_menu(lang),
                Some("language") => language_menu(lang),
                Some("admin") => {
                    if !is_admin {
                        let msg = match lang {
                            Language::English => "Admin only menu.",
                            Language::Spanish => "Menú solo admin.",
                            Language::Chinese => "仅管理员可用菜单。",
                            Language::Russian => "Меню только для админа.",
                            Language::Italian => "Menu solo admin.",
                        };
                        return Some(action_message(
                            chat_id,
                            account_tag,
                            message_id,
                            msg.to_string(),
                            Some(vec![vec![nav_back(lang)]]),
                        ));
                    }
                    admin_menu(lang)
                }
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
            let msg = match parts.get(1).copied() {
                Some("add") => match lang {
                    Language::English => "➕ *New Reminder*\n\nParser examples:\n`/cron domani alle 9 \"Meeting\"`\n`/cron tra 30 minuti \"Break\"`\n`/cron ogni lunedi alle 8 \"News\"`",
                    Language::Spanish => "➕ *Nuevo Recordatorio*\n\nEjemplos parser:\n`/cron domani alle 9 \"Meeting\"`\n`/cron tra 30 minuti \"Break\"`\n`/cron ogni lunedi alle 8 \"News\"`",
                    Language::Chinese => "➕ *新提醒*\n\nParser 示例：\n`/cron domani alle 9 \"Meeting\"`\n`/cron tra 30 minuti \"Break\"`\n`/cron ogni lunedi alle 8 \"News\"`",
                    Language::Russian => "➕ *Новое напоминание*\n\nПримеры parser:\n`/cron domani alle 9 \"Meeting\"`\n`/cron tra 30 minuti \"Break\"`\n`/cron ogni lunedi alle 8 \"News\"`",
                    Language::Italian => "➕ *Nuovo Promemoria*\n\nEsempi parser:\n`/cron domani alle 9 \"Meeting\"`\n`/cron tra 30 minuti \"Break\"`\n`/cron ogni lunedi alle 8 \"News\"`",
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
            let msg = match parts.get(1).copied() {
                Some("fs") => match lang {
                    Language::English => "📁 *Filesystem*\n\nAsk me to read or write files.",
                    Language::Spanish => "📁 *Archivos*\n\nPídeme leer o escribir archivos.",
                    Language::Chinese => "📁 *文件系统*\n\n让我读写文件。",
                    Language::Russian => "📁 *Файлы*\n\nПопросите меня читать или писать файлы.",
                    Language::Italian => "📁 *File*\n\nChiedimi di leggere o scrivere file.",
                },
                Some("exec") => match lang {
                    _ if !is_admin => "🖥️ Admin only command.",
                    Language::English => "🖥️ *Exec*\n\nRun commands: `/exec ls -la`",
                    Language::Spanish => "🖥️ *Ejecutar*\n\nEjecuta comandos: `/exec ls -la`",
                    Language::Chinese => "🖥️ *执行*\n\n运行命令: `/exec ls -la`",
                    Language::Russian => "🖥️ *Выполнить*\n\nЗапустите команды: `/exec ls -la`",
                    Language::Italian => "🖥️ *Esegui*\n\nEsegui comandi: `/exec ls -la`",
                },
                Some("termux") => match lang {
                    Language::English => {
                        "📱 *Termux*\n\nCommands: `/termux battery`, `/termux info`, `/termux wake status`"
                    }
                    Language::Spanish => {
                        "📱 *Termux*\n\nComandos: `/termux battery`, `/termux info`, `/termux wake status`"
                    }
                    Language::Chinese => {
                        "📱 *Termux*\n\n命令: `/termux battery`, `/termux info`, `/termux wake status`"
                    }
                    Language::Russian => {
                        "📱 *Termux*\n\nКоманды: `/termux battery`, `/termux info`, `/termux wake status`"
                    }
                    Language::Italian => {
                        "📱 *Termux*\n\nComandi: `/termux battery`, `/termux info`, `/termux wake status`"
                    }
                },
                _ => return None,
            };
            let (_, menu_keyboard) = utility_menu(lang, is_admin);
            Some(action_message(
                chat_id,
                account_tag,
                message_id,
                msg.to_string(),
                Some(menu_keyboard),
            ))
        }
        "admin" => {
            if !is_admin {
                let msg = match lang {
                    Language::English => "Admin only command.",
                    Language::Spanish => "Comando solo admin.",
                    Language::Chinese => "仅管理员命令。",
                    Language::Russian => "Команда только для админа.",
                    Language::Italian => "Comando solo admin.",
                };
                return Some(action_message(
                    chat_id,
                    account_tag,
                    message_id,
                    msg.to_string(),
                    Some(vec![vec![nav_back(lang)]]),
                ));
            }

            let msg = match parts.get(1).copied() {
                Some("acl") => "👥 *ACL*\n\n`/admin list`\n`/admin add <user_id>`\n`/admin remove <user_id>`\n`/admin promote <user_id>`\n`/admin demote <user_id>`",
                Some("user_tools") => "🧰 *User Tools Policy*\n\n`/admin tools user list`\n`/admin tools user available`\n`/admin tools user mode <none|selected>`\n`/admin tools user allow <tool_name>`\n`/admin tools user deny <tool_name>`\n`/admin tools user clear`",
                Some("runtime") => "🔌 *Runtime*\n\n`/plugin` - module catalog + key management\n`/mcp` - MCP status\n`/tools` - runtime tool list",
                Some("exec") => "🖥️ *Exec*\n\n`/exec <command>`\nRuns only allowlisted commands.",
                _ => return None,
            };

            let (_, menu_keyboard) = admin_menu(lang);
            Some(action_message(
                chat_id,
                account_tag,
                message_id,
                msg.to_string(),
                Some(menu_keyboard),
            ))
        }
        "settings" => {
            let msg = match parts.get(1).copied() {
                Some("help") => help_text(lang, is_admin),
                _ => return None,
            };
            let (_, menu_keyboard) = settings_menu(lang);
            Some(action_message(
                chat_id,
                account_tag,
                message_id,
                msg,
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
        let (text, keyboard) = home_menu(Language::English, false);
        assert!(text.contains("Masix Bot"));
        assert!(!keyboard.is_empty());
    }

    #[test]
    fn test_language_parse() {
        assert_eq!("en".parse::<Language>().unwrap(), Language::English);
        assert_eq!("it".parse::<Language>().unwrap(), Language::Italian);
    }
}
