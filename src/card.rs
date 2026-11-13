use crate::dates::CardDate;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Card {
    pub kind: String,
    pub organization: String,
    pub inn: String,
    pub kpp: String,
    pub ogrn: String,
    pub model: String,
    pub serial: String,
    pub registration: String,
    pub fn_model: String,
    pub fn_serial: String,
    pub address: String,
    pub place: String,
    pub registered_at: String,
    pub changed_at: String,
    pub deregistered_at: String,
    #[serde(default)]
    pub formed_at: String,
    pub changes: String,
    pub ofd: String,
    pub ofd_inn: String,
    pub flags: Vec<(String, String)>,
    pub signature: Vec<(String, String)>,
}

/// Keep the stored RNM as digits, accepting spaces pasted from the UI or PDF.
pub fn compact_registration(value: &str) -> String {
    value.chars().filter(|c| !c.is_whitespace()).collect()
}

/// Display and copy RNM in groups of four without losing leading zeros.
pub fn format_registration(value: &str) -> String {
    let compact = compact_registration(value);
    if !compact.bytes().all(|b| b.is_ascii_digit()) {
        return value.to_string();
    }
    let mut formatted = String::with_capacity(compact.len() + 3);
    for (i, digit) in compact.chars().enumerate() {
        if i > 0 && i % 4 == 0 {
            formatted.push(' ');
        }
        formatted.push(digit);
    }
    formatted
}

static RULES: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"(?is)(?:принадлеж(?:ит|ащая|ащий)\s*:?|пользователь\s*:)\s*(.+?)\s*,?\s*(?:ОГРН(?:ИП)?|ИНН(?:\s*/\s*КПП)?)\b",
        r"(?is)\bИНН\s*(?:/\s*КПП)?\s*:?\s*([0-9]{12}|[0-9]{10})\b",
        r"(?is)ИНН\s*/\s*КПП\s*:?\s*[0-9\s]+/\s*([0-9]{9})",
        r"(?is)\bОГРН(?:ИП)?\s*:?\s*([0-9]{15}|[0-9]{13})\b",
        r"(?im)^\s*контрольно\s*[-–]?\s*кассовая\s+техника\s+(.+?)\s*$",
        r"(?is)заводской\s+номер\s+(?:(?:экземпляра\s+)?(?:модели\s+)?контрольно\s*[-–]?\s*кассовой\s+техники|ККТ)\s*:?\s*([0-9A-Za-zА-Яа-я_-]+)",
        r"(?is)регистрационный\s+номер\s+(?:контрольно\s*[-–]?\s*кассовой\s+техники|ККТ)\s*:?\s*([0-9](?:\s*[0-9]){15})\b",
        r"(?is)модель\s+фискального\s+накопителя\s*:\s*(.+?)\s*заводской\s+номер",
        r"(?is)заводской\s+номер\s+экземпляра\s+(?:модели\s+)?фискального\s+накопителя\s*:?\s*([0-9]{10,20})",
        r"(?is)адрес\s+установки\s*\(применения\)\s*контрольно\s*[-–]?\s*кассовой\s+техники\s*:\s*(.+?)\s*место\s+установки",
        r"(?is)место\s+установки\s*\(применения\)\s*контрольно\s*[-–]?\s*кассовой\s+техники\s*:\s*(.+?)\s*(?:входит\s+в\s+состав|пользователь\s+является|используется|номер\s+автоматического)",
        r"(?is)зарегистрирована\s+в\s+налоговом\s+органе\s+с\s*(\d{2}\.\d{2}\.\d{4})",
        r"(?is)дата\s+последнего\s+внесения\s+изменений[^:]*:\s*(\d{2}\.\d{2}\.\d{4})",
        r"(?is)(?:снят[аоы]?\s+с\s+(?:регистрационного\s+)?уч[её]та(?:\s+в\s+налоговом\s+органе)?\s*(?:с|от)?\s*:?|дата\s+снятия(?:\s+с\s+(?:регистрационного\s+)?уч[её]та)?\s*:?)\s*(\d{2}[./-]\d{2}[./-]\d{4}|\d{4}-\d{2}-\d{2})",
        r"(?im)количество\s+перерегистраций\s+контрольно\s*[-–]?\s*кассовой\s+техники[ \t:]+([0-9]{1,4})[ \t]*$",
        r"(?is)оператор\s+фискальных\s+данных\s+пользователя\s*:?\s*(.*?)\s*,?\s*ИНН\s*:?\s*\d{10,12}",
        r"(?is)оператор\s+фискальных\s+данных\s+пользователя.*?\bИНН\s*:?\s*(\d{10,12})",
    ].into_iter().map(|s| Regex::new(s).unwrap()).collect()
});

fn clean(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches([',', ':', ' '])
        .to_string()
}
fn digits(s: String) -> String {
    s.chars().filter(char::is_ascii_digit).collect()
}

impl Card {
    pub fn parse(text: &str) -> Self {
        // Text is data only. No document content is ever executed or interpreted as a command.
        let text = text
            .replace('\r', "\n")
            .replace('\u{a0}', " ")
            .replace('\u{ad}', "")
            .replace(['\u{2010}', '\u{2011}', '\u{2212}'], "-");
        let cap = |i: usize| {
            RULES[i]
                .captures(&text)
                .map(|c| clean(&c[1]))
                .unwrap_or_default()
        };
        let lower = clean(&text).to_lowercase();
        static REMOVAL: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"(?is)\bкарточка\s+(?:о\s+)?(?:снятии|снятия|снятие)\b|\bснят[аоы]?\s+с\s+(?:регистрационного\s+)?уч[её]та\b").unwrap()
        });
        let mut card = Self {
            kind: if REMOVAL.is_match(&lower) {
                "Снятие с учёта"
            } else if lower.contains("карточка регистрации") {
                "Регистрация"
            } else {
                "Не определён"
            }
            .into(),
            organization: cap(0),
            inn: digits(cap(1)),
            kpp: cap(2),
            ogrn: digits(cap(3)),
            model: cap(4).trim_end_matches('.').to_string(),
            serial: cap(5),
            registration: digits(cap(6)),
            fn_model: cap(7),
            fn_serial: cap(8),
            address: cap(9),
            place: cap(10),
            registered_at: cap(11),
            changed_at: cap(12),
            deregistered_at: canonical_date(&cap(13)),
            changes: cap(14),
            ofd: cap(15),
            ofd_inn: cap(16),
            ..Default::default()
        };
        if ![10, 12].contains(&card.inn.len()) {
            card.inn.clear();
        }
        if ![13, 15].contains(&card.ogrn.len()) {
            card.ogrn.clear();
        }
        if !card.serial.chars().any(|c| c.is_ascii_digit()) {
            card.serial.clear();
        }
        static MODEL: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"(?is)(?:^|\n)\s*контрольно\s*[-–]?\s*кассовая\s+техника\s+(.+?)\s*заводской\s+номер").unwrap()
        });
        if let Some(c) = MODEL.captures(&text) {
            card.model = clean(&c[1]).trim_end_matches('.').to_string();
        }
        if card.registration.len() != 16 {
            card.registration.clear();
        }
        if card.registration.is_empty() {
            static HEADER: LazyLock<Regex> = LazyLock::new(|| {
                Regex::new(r"(?is)(?:техники|уч[её]та)\s*№\s*([0-9](?:\s*[0-9]){15})\b").unwrap()
            });
            if let Some(c) = HEADER.captures(&text) {
                let n = digits(c[1].to_string());
                if n.len() == 16 {
                    card.registration = n;
                }
            }
        }
        static FLAGS: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"(?is)((?:входит\s+в\s+состав|пользователь\s+является|используется|применяется|контрольно[-–]?кассовая\s+техника).*?)\(да\s*/\s*нет\)\s*(да|нет)\b").unwrap()
        });
        // Split after each answer so a long, wrapped label stays attached to its own value.
        let offset = text
            .find("Входит в состав")
            .or_else(|| text.find("Пользователь является"))
            .unwrap_or(text.len());
        for c in FLAGS.captures_iter(&text[offset..]) {
            let label = clean(&c[1]);
            if !label.is_empty() {
                card.flags.push((label, clean(&c[2])));
            }
        }
        for (label, pattern) in [
            (
                "Сертификат электронной подписи",
                r"(?im)сертификат\s*:\s*([^\n]+)",
            ),
            ("Владелец сертификата", r"(?im)владелец\s*:\s*([^\n]+)"),
            (
                "Срок действия сертификата",
                r"(?im)действителен[ \t]*:[ \t]*([^\n]+)",
            ),
        ] {
            if let Some(c) = Regex::new(pattern).unwrap().captures(&text) {
                let value = clean(&c[1]);
                if !value.is_empty() {
                    card.signature.push((label.into(), value));
                }
            }
        }
        card.formed_at = formation_date(&text, &card)
            .map(CardDate::russian)
            .unwrap_or_default();
        card
    }

    pub fn suggested_filename(&self) -> String {
        let date = self.date().map(CardDate::iso).unwrap_or_default();
        let kind = if self.is_deregistration() {
            "Снятие"
        } else if self.kind == "Регистрация" {
            "Регистрация"
        } else {
            "Карточка ККТ"
        };
        let mut parts = vec![date, kind.to_string()];
        if !self.registration.is_empty() {
            parts.push(format!("РН {}", self.registration));
        }
        if !self.serial.is_empty() {
            parts.push(format!("ЗН {}", self.serial));
        }
        format!(
            "{}.pdf",
            parts
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" ")
        )
    }

    pub fn is_deregistration(&self) -> bool {
        self.kind == "Снятие с учёта"
    }

    pub fn date(&self) -> Option<CardDate> {
        CardDate::parse(&self.formed_at)
    }

    pub fn recognized(&self) -> bool {
        self.kind == "Регистрация" || self.is_deregistration()
    }

    /// Deregistration cards normally omit FN, address and OFD data. Show fields
    /// that belong to the form, plus any additional values actually present.
    pub fn display_fields(&self) -> Vec<(&'static str, &str)> {
        self.fields()
            .into_iter()
            .filter(|(label, value)| {
                !self.is_deregistration()
                    || !value.is_empty()
                    || matches!(
                        *label,
                        "Тип карточки"
                            | "Организация / ИП"
                            | "ИНН"
                            | "Модель ККТ"
                            | "Заводской номер ККТ"
                            | "Регистрационный номер ККТ"
                            | "Дата снятия с учёта"
                    )
            })
            .collect()
    }

    pub fn fields(&self) -> Vec<(&'static str, &str)> {
        vec![
            ("Тип карточки", &self.kind),
            ("Организация / ИП", &self.organization),
            ("ИНН", &self.inn),
            ("КПП", &self.kpp),
            ("ОГРН / ОГРНИП", &self.ogrn),
            ("Модель ККТ", &self.model),
            ("Заводской номер ККТ", &self.serial),
            ("Регистрационный номер ККТ", &self.registration),
            ("Модель ФН", &self.fn_model),
            ("Заводской номер ФН", &self.fn_serial),
            ("Адрес установки", &self.address),
            ("Место установки", &self.place),
            ("Дата регистрации", &self.registered_at),
            ("Последнее изменение", &self.changed_at),
            ("Дата снятия с учёта", &self.deregistered_at),
            ("Дата карточки", &self.formed_at),
            ("Число перерегистраций", &self.changes),
            ("Оператор фискальных данных", &self.ofd),
            ("ИНН ОФД", &self.ofd_inn),
        ]
    }
}

fn formation_date(text: &str, card: &Card) -> Option<CardDate> {
    static FORMATION: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
        r"(?is)(?:дата\s+(?:формирования|выдачи|составления)(?:\s+карточки)?|карточка\s+сформирована)\s*[:–-]?\s*(\d{2}[./-]\d{2}[./-]\d{4}|\d{4}-\d{2}-\d{2})"
    ).unwrap()
    });
    static FOOTER: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?m)^\s*(\d{2}\.\d{2}\.\d{4})\s*$").unwrap());
    static REPORT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
        r"(?is)дата\s+последнего\s+отч[её]та\s+о\s+регистрации[^:]{0,180}:\s*(\d{2}\.\d{2}\.\d{4})"
    ).unwrap()
    });
    if let Some(date) = FORMATION
        .captures(text)
        .and_then(|c| CardDate::parse(&c[1]))
    {
        return Some(date);
    }
    // The issued date is a separate footer after the signature, never the two
    // dates on the certificate-validity line or the original registration date.
    static VALIDITY: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"(?is)действителен\s*:\s*[cс]\s*\d{2}\.\d{2}\.\d{4}\s*(?:до|по)\s*\d{2}\.\d{2}\.\d{4}",
        )
        .unwrap()
    });
    if let Some(validity) = VALIDITY.find_iter(text).last() {
        let dates: Vec<_> = FOOTER
            .captures_iter(&text[validity.end()..])
            .filter_map(|c| CardDate::parse(&c[1]))
            .collect();
        if let Some(date) = dates.last() {
            return Some(*date);
        }
    }
    // Some issued forms leave the certificate-validity value entirely blank.
    // Their sole final standalone date is still printed below the signature box.
    static BLANK_VALIDITY_FOOTER: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?im)действителен[ \t]*:[ \t]*\n[ \t\n]*(\d{2}\.\d{2}\.\d{4})[ \t\n]*\z")
            .unwrap()
    });
    if let Some(date) = BLANK_VALIDITY_FOOTER
        .captures(text)
        .and_then(|c| CardDate::parse(&c[1]))
    {
        return Some(date);
    }
    if card.is_deregistration() {
        return CardDate::parse(&card.deregistered_at);
    }
    REPORT
        .captures(text)
        .and_then(|c| CardDate::parse(&c[1]))
        .or_else(|| CardDate::parse(&card.changed_at))
}

fn canonical_date(value: &str) -> String {
    let parts: Vec<_> = value.split(['.', '/', '-']).collect();
    if parts.len() != 3 {
        return value.to_string();
    }
    if parts[0].len() == 4 {
        format!("{}.{}.{}", parts[2], parts[1], parts[0])
    } else {
        parts.join(".")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registration_display_preserves_digits_and_storage_names() {
        let compact = "0008541108014111";
        let formatted = "0008 5411 0801 4111";
        for kind in ["Регистрация", "Снятие с учёта"] {
            let card = Card {
                kind: kind.into(),
                registration: compact.into(),
                ..Default::default()
            };
            assert_eq!(format_registration(&card.registration), formatted);
            assert_eq!(compact_registration(formatted), card.registration);
            assert!(card.suggested_filename().contains("РН 0008541108014111"));
        }
        assert_eq!(format_registration("0008\u{a0}5411\t0801\n4111"), formatted);
        assert_eq!(format_registration(formatted), formatted);
        assert_eq!(format_registration(""), "");
        // Invalid input must reach RNM validation, not silently lose symbols.
        assert_eq!(
            compact_registration("0008 5411 0801 41x1"),
            "00085411080141x1"
        );
    }

    #[test]
    fn formation_footer_wins_over_original_registration_and_certificate() {
        let c = Card::parse(
            "КАРТОЧКА РЕГИСТРАЦИИ\nЗарегистрирована в налоговом органе с 15.07.2021\nДата последнего отчета о регистрации (отчета об изменении параметров регистрации):24.12.2025\nДОКУМЕНТ ПОДПИСАН\nДействителен: c 06.04.2024 до 06.04.2026\n24.12.2025\n",
        );
        assert_eq!(c.formed_at, "24.12.2025");
        assert!(c.suggested_filename().starts_with("2025-12-24"));
        let old = Card::parse(
            "КАРТОЧКА РЕГИСТРАЦИИ\nЗарегистрирована в налоговом органе с 25.11.2016\nДействителен: c 06.04.2019 до 06.04.2020\n26.07.2019\n",
        );
        assert_eq!(old.formed_at, "26.07.2019");
        let unknown = Card::parse(
            "КАРТОЧКА РЕГИСТРАЦИИ\nЗарегистрирована в налоговом органе с 25.11.2016\nДействителен: c 06.04.2019 до 06.04.2020\n",
        );
        assert!(unknown.formed_at.is_empty());
        let wrapped = Card::parse(
            "КАРТОЧКА РЕГИСТРАЦИИ\nЗарегистрирована в налоговом органе с 25.11.2016\nДействителен: c 06.04.2019 до\n06.04.2020\n",
        );
        assert!(
            wrapped.formed_at.is_empty(),
            "Certificate date must not become the card date"
        );
        let blank =
            Card::parse("КАРТОЧКА РЕГИСТРАЦИИ\nДОКУМЕНТ ПОДПИСАН\nДействителен: \n10.04.2018\n");
        assert_eq!(blank.formed_at, "10.04.2018");
        assert!(blank.signature.is_empty());
    }
    #[test]
    fn deregistration_ip_wrapped_and_yo() {
        let c = Card::parse(
            "КАРТОЧКА О\nСНЯТИИ КОНТРОЛЬНО-КАССОВОЙ ТЕХНИКИ С\nРЕГИСТРАЦИОННОГО УЧЁТА № 0002 7952 4602 7711\nКонтрольно-кассовая техника ЭВОТОР\nСТ2Ф.\nЗаводской номер экземпляра контрольно-кассовой техники: 00307403092560,\nПринадлежащая ИВАНОВ ИВАН ИВАНОВИЧ, ОГРНИП 318121500023611, ИНН\n120500391485\nСнята с регистрационного учёта в налоговом органе с\n13.01.2020\n(дата снятия)",
        );
        assert!(c.is_deregistration());
        assert_eq!(c.model, "ЭВОТОР СТ2Ф");
        assert_eq!(c.inn, "120500391485");
        assert_eq!(c.ogrn, "318121500023611");
        assert_eq!(c.registration, "0002795246027711");
        assert_eq!(c.deregistered_at, "13.01.2020");
        assert!(c.suggested_filename().starts_with("2020-01-13 Снятие РН"));
        assert!(!c.display_fields().iter().any(|(k, _)| *k == "Модель ФН"));
        assert!(
            c.display_fields()
                .iter()
                .any(|(k, _)| *k == "Дата снятия с учёта")
        );
    }
    #[test]
    fn deregistration_short_form_and_iso_date() {
        let c = Card::parse(
            "КАРТОЧКА СНЯТИЯ С УЧЁТА\nКонтрольно-кассовая техника АТОЛ 30Ф.\nЗаводской номер ККТ: 1234567\nРегистрационный номер ККТ: 0000000000001234\nПользователь: ООО «ПРИМЕР», ИНН 1200016105\nДата снятия с учёта: 2025-08-15",
        );
        assert!(c.is_deregistration());
        assert_eq!(c.organization, "ООО «ПРИМЕР»");
        assert_eq!(c.inn, "1200016105");
        assert_eq!(c.serial, "1234567");
        assert_eq!(c.deregistered_at, "15.08.2025");
        assert!(c.suggested_filename().starts_with("2025-08-15 Снятие"));
    }
    #[test]
    fn unrecognized_document_not_named_registration() {
        let c = Card::parse("Произвольный документ без реквизитов карточки");
        assert_eq!(c.kind, "Не определён");
        assert_eq!(c.suggested_filename(), "Карточка ККТ.pdf");
    }
    #[test]
    fn wrapped_flags_and_table_values_stay_separate() {
        let c = Card::parse(
            "КАРТОЧКА\nРЕГИСТРАЦИИ\nВходит в состав автоматического устройства\nдля расчетов (да/нет) Нет\nНомер автоматического устройства\nПользователь является банковским платежным агентом (да/нет) Нет\nИспользуется для развозной\nторговли (да/нет) Да\nКоличество перерегистраций контрольно-кассовой техники 2\nЗарегистрирована в налоговом органе с 18.05.2017\n(дата регистрации)",
        );
        assert_eq!(c.kind, "Регистрация");
        assert_eq!(c.flags.len(), 3);
        assert_eq!(c.flags[0].1, "Нет");
        assert!(c.flags[1].0.starts_with("Пользователь"));
        assert_eq!(c.flags[2].1, "Да");
        assert_eq!(c.changes, "2");
        assert_eq!(c.registered_at, "18.05.2017");
    }
    #[test]
    fn deregistration_multiline() {
        let c = Card::parse(
            "КАРТОЧКА О СНЯТИИ\nКонтрольно-кассовая техника РИТЕЙЛ-01ФМ.\nЗаводской номер экземпляра контрольно-кассовой техники: 0614850012279840,\nрегистрационный номер контрольно-кассовой техники 0008 5411 0801 4111.\nПринадлежащая ОБЩЕСТВО С ОГРАНИЧЕННОЙ ОТВЕТСТВЕННОСТЬЮ \"ПРИМЕР\",\nОГРН 1241200002955, ИНН/КПП 1200016105/120001001\nСнята с регистрационного учета в налоговом органе с 06.02.2025",
        );
        assert_eq!(c.inn, "1200016105");
        assert_eq!(c.ogrn, "1241200002955");
        assert_eq!(c.registration, "0008541108014111");
        assert_eq!(c.serial, "0614850012279840");
        assert!(c.organization.ends_with("\"ПРИМЕР\""));
        assert_eq!(c.deregistered_at, "06.02.2025");
    }
    #[test]
    fn ip_and_ofd_inn_are_separate() {
        let c = Card::parse(
            "КАРТОЧКА РЕГИСТРАЦИИ\nПринадлежит: ИВАНОВ ИВАН ИВАНОВИЧ, ОГРНИП 321120000011040, ИНН\n121527087685\nАдрес установки (применения) контрольно-кассовой техники: дом 2\nМесто установки (применения) контрольно-кассовой техники: магазин\nВходит в состав устройства (да/нет) Нет\nОператор фискальных данных пользователя ООО ОФД, ИНН 9715260691",
        );
        assert_eq!(c.inn, "121527087685");
        assert_eq!(c.ofd_inn, "9715260691");
        assert_eq!(c.address, "дом 2");
        assert_eq!(c.place, "магазин");
        assert_eq!(c.flags.len(), 1);
    }
}
