//! Multi-language support for gateway messages.
//!
//! Supported languages: English (en), Latvian (lv), Russian (ru).
//! All UI strings are in English by default, with i18n overrides.

use once_cell::sync::Lazy;
use std::collections::HashMap;

/// Supported languages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Lv,
    Ru,
}

impl Lang {
    /// Detect language from a Telegram language_code or similar.
    pub fn from_code(code: Option<&str>) -> Self {
        match code.map(|c| c.split('-').next().unwrap_or(c).to_lowercase()) {
            Some(ref c) if c == "lv" => Lang::Lv,
            Some(ref c) if c == "ru" => Lang::Ru,
            _ => Lang::En,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Lv => "lv",
            Lang::Ru => "ru",
        }
    }
}

/// Keyed translation store.
#[derive(Debug)]
pub struct I18n {
    lang: Lang,
}

impl I18n {
    pub fn new(lang: Lang) -> Self {
        Self { lang }
    }

    /// Get a translated string by key.
    pub fn t(&self, key: &str) -> String {
        TRANSLATIONS
            .get(key)
            .and_then(|tr| match self.lang {
                Lang::En => Some(tr.en.clone()),
                Lang::Lv => tr.lv.clone(),
                Lang::Ru => tr.ru.clone(),
            })
            .unwrap_or_else(|| key.to_string())
    }

    /// Format a translated string with arguments.
    pub fn tf(&self, key: &str, args: &[(&str, &str)]) -> String {
        let mut s = self.t(key);
        for (k, v) in args {
            s = s.replace(&format!("{{{k}}}"), v);
        }
        s
    }
}

/// Default English-only I18n.
impl Default for I18n {
    fn default() -> Self {
        Self { lang: Lang::En }
    }
}

struct Translation {
    en: String,
    lv: Option<String>,
    ru: Option<String>,
}

static TRANSLATIONS: Lazy<HashMap<&'static str, Translation>> = Lazy::new(|| {
    let mut m = HashMap::new();

    // ---- Greetings ----
    m.insert(
        "greeting",
        Translation {
            en: "Hello! I'm ohAgent, your 24/7 AI assistant.\n\nI can help you with coding, research, scheduling, and more. Just tell me what you need.".into(),
            lv: Some("Sveiki! Es esmu ohAgent — jūsu 24/7 AI asistents.\n\nEs varu palīdzēt ar programmēšanu, pētniecību, plānošanu un daudz ko citu. Vienkārši pasakiet, kas nepieciešams.".into()),
            ru: Some("Здравствуйте! Я ohAgent — ваш круглосуточный AI-ассистент.\n\nЯ могу помочь с программированием, исследованиями, планированием и многим другим. Просто скажите, что нужно.".into()),
        },
    );

    // ---- Pairing ----
    m.insert(
        "pairing_code_sent",
        Translation {
            en: "Your pairing code: `{code}`\nIt expires in {minutes} minutes.".into(),
            lv: Some("Jūsu savienošanas kods: `{code}`\nTas ir derīgs {minutes} minūtes.".into()),
            ru: Some("Ваш код сопряжения: `{code}`\nДействителен {minutes} минут.".into()),
        },
    );

    m.insert(
        "pairing_success",
        Translation {
            en: "Pairing successful! You are now connected.".into(),
            lv: Some("Savienošana veiksmīga! Jūs esat pieslēdzies.".into()),
            ru: Some("Сопряжение успешно! Вы подключены.".into()),
        },
    );

    m.insert(
        "not_paired",
        Translation {
            en: "You are not paired yet. Use `/pair` to get started.".into(),
            lv: Some("Jūs vēl neesat savienots. Izmantojiet `/pair`, lai sāktu.".into()),
            ru: Some("Вы ещё не сопряжены. Используйте `/pair`, чтобы начать.".into()),
        },
    );

    // ---- Status ----
    m.insert(
        "thinking",
        Translation {
            en: "Thinking...".into(),
            lv: Some("Domāju...".into()),
            ru: Some("Думаю...".into()),
        },
    );

    m.insert(
        "done",
        Translation {
            en: "Done.".into(),
            lv: Some("Gatavs.".into()),
            ru: Some("Готово.".into()),
        },
    );

    m.insert(
        "error",
        Translation {
            en: "Sorry, something went wrong: {error}".into(),
            lv: Some("Atvainojiet, radās kļūda: {error}".into()),
            ru: Some("Извините, произошла ошибка: {error}".into()),
        },
    );

    m.insert(
        "rate_limited",
        Translation {
            en: "You're sending messages too fast. Please wait a moment.".into(),
            lv: Some("Jūs sūtāt ziņojumus pārāk ātri. Lūdzu, uzgaidiet mirkli.".into()),
            ru: Some("Вы отправляете сообщения слишком быстро. Пожалуйста, подождите.".into()),
        },
    );

    // ---- Commands ----
    m.insert(
        "help",
        Translation {
            en: "*ohAgent Commands*\n\n/start — Start the bot\n/pair — Pair your account\n/status — Check agent status\n/stop — Stop current task\n/new — Start a new conversation\n/lang — Change language\n/help — Show this help".into(),
            lv: Some("*ohAgent Komandas*\n\n/start — Sākt botu\n/pair — Savienot kontu\n/status — Pārbaudīt aģenta statusu\n/stop — Apturēt pašreizējo uzdevumu\n/new — Sākt jaunu sarunu\n/lang — Mainīt valodu\n/help — Parādīt šo palīdzību".into()),
            ru: Some("*ohAgent Команды*\n\n/start — Запустить бота\n/pair — Сопрячь аккаунт\n/status — Проверить статус агента\n/stop — Остановить текущую задачу\n/new — Начать новый разговор\n/lang — Сменить язык\n/help — Показать эту справку".into()),
        },
    );

    m.insert(
        "new_session",
        Translation {
            en: "Starting a fresh conversation.".into(),
            lv: Some("Sāku jaunu sarunu.".into()),
            ru: Some("Начинаю новый разговор.".into()),
        },
    );

    m.insert(
        "lang_changed",
        Translation {
            en: "Language set to English.".into(),
            lv: Some("Valoda iestatīta uz latviešu.".into()),
            ru: Some("Язык изменён на русский.".into()),
        },
    );

    m.insert(
        "task_stopped",
        Translation {
            en: "Current task stopped.".into(),
            lv: Some("Pašreizējais uzdevums apturēts.".into()),
            ru: Some("Текущая задача остановлена.".into()),
        },
    );

    m
});
