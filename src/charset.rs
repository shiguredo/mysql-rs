// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

//! MySQL 文字セット定義。

use std::collections::HashMap;
use std::sync::LazyLock;

#[derive(Debug, Clone)]
pub struct Charset {
    pub id: u16,
    pub name: String,
    pub collation: String,
    pub is_default: bool,
}

impl Charset {
    pub fn encoding(&self) -> &str {
        match self.name.as_str() {
            "utf8mb4" | "utf8mb3" => "utf8",
            "latin1" => "cp1252",
            "koi8r" => "koi8_r",
            "koi8u" => "koi8_u",
            other => other,
        }
    }
}

pub struct Charsets {
    by_id: HashMap<u16, Charset>,
    by_name: HashMap<String, Charset>,
}

impl Charsets {
    fn new() -> Self {
        Self {
            by_id: HashMap::new(),
            by_name: HashMap::new(),
        }
    }

    fn add(&mut self, c: Charset) {
        self.by_id.insert(c.id, c.clone());
        if c.is_default {
            self.by_name.insert(c.name.clone(), c);
        }
    }

    pub fn by_id(&self, id: u16) -> Option<&Charset> {
        self.by_id.get(&id)
    }

    pub fn by_name(&self, name: &str) -> Option<&Charset> {
        let lowered = name.to_lowercase();
        let key = if lowered == "utf8" {
            "utf8mb4"
        } else {
            lowered.as_str()
        };
        self.by_name.get(key)
    }
}

static CHARSETS: LazyLock<Charsets> = LazyLock::new(|| {
    let mut c = Charsets::new();
    c.add(Charset {
        id: 1,
        name: "big5".to_string(),
        collation: "big5_chinese_ci".to_string(),
        is_default: true,
    });
    c.add(Charset {
        id: 2,
        name: "latin2".to_string(),
        collation: "latin2_czech_cs".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 3,
        name: "dec8".to_string(),
        collation: "dec8_swedish_ci".to_string(),
        is_default: true,
    });
    c.add(Charset {
        id: 4,
        name: "cp850".to_string(),
        collation: "cp850_general_ci".to_string(),
        is_default: true,
    });
    c.add(Charset {
        id: 5,
        name: "latin1".to_string(),
        collation: "latin1_german1_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 6,
        name: "hp8".to_string(),
        collation: "hp8_english_ci".to_string(),
        is_default: true,
    });
    c.add(Charset {
        id: 7,
        name: "koi8r".to_string(),
        collation: "koi8r_general_ci".to_string(),
        is_default: true,
    });
    c.add(Charset {
        id: 8,
        name: "latin1".to_string(),
        collation: "latin1_swedish_ci".to_string(),
        is_default: true,
    });
    c.add(Charset {
        id: 9,
        name: "latin2".to_string(),
        collation: "latin2_general_ci".to_string(),
        is_default: true,
    });
    c.add(Charset {
        id: 10,
        name: "swe7".to_string(),
        collation: "swe7_swedish_ci".to_string(),
        is_default: true,
    });
    c.add(Charset {
        id: 11,
        name: "ascii".to_string(),
        collation: "ascii_general_ci".to_string(),
        is_default: true,
    });
    c.add(Charset {
        id: 12,
        name: "ujis".to_string(),
        collation: "ujis_japanese_ci".to_string(),
        is_default: true,
    });
    c.add(Charset {
        id: 13,
        name: "sjis".to_string(),
        collation: "sjis_japanese_ci".to_string(),
        is_default: true,
    });
    c.add(Charset {
        id: 14,
        name: "cp1251".to_string(),
        collation: "cp1251_bulgarian_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 15,
        name: "latin1".to_string(),
        collation: "latin1_danish_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 16,
        name: "hebrew".to_string(),
        collation: "hebrew_general_ci".to_string(),
        is_default: true,
    });
    c.add(Charset {
        id: 18,
        name: "tis620".to_string(),
        collation: "tis620_thai_ci".to_string(),
        is_default: true,
    });
    c.add(Charset {
        id: 19,
        name: "euckr".to_string(),
        collation: "euckr_korean_ci".to_string(),
        is_default: true,
    });
    c.add(Charset {
        id: 20,
        name: "latin7".to_string(),
        collation: "latin7_estonian_cs".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 21,
        name: "latin2".to_string(),
        collation: "latin2_hungarian_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 22,
        name: "koi8u".to_string(),
        collation: "koi8u_general_ci".to_string(),
        is_default: true,
    });
    c.add(Charset {
        id: 23,
        name: "cp1251".to_string(),
        collation: "cp1251_ukrainian_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 24,
        name: "gb2312".to_string(),
        collation: "gb2312_chinese_ci".to_string(),
        is_default: true,
    });
    c.add(Charset {
        id: 25,
        name: "greek".to_string(),
        collation: "greek_general_ci".to_string(),
        is_default: true,
    });
    c.add(Charset {
        id: 26,
        name: "cp1250".to_string(),
        collation: "cp1250_general_ci".to_string(),
        is_default: true,
    });
    c.add(Charset {
        id: 27,
        name: "latin2".to_string(),
        collation: "latin2_croatian_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 28,
        name: "gbk".to_string(),
        collation: "gbk_chinese_ci".to_string(),
        is_default: true,
    });
    c.add(Charset {
        id: 29,
        name: "cp1257".to_string(),
        collation: "cp1257_lithuanian_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 30,
        name: "latin5".to_string(),
        collation: "latin5_turkish_ci".to_string(),
        is_default: true,
    });
    c.add(Charset {
        id: 31,
        name: "latin1".to_string(),
        collation: "latin1_german2_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 32,
        name: "armscii8".to_string(),
        collation: "armscii8_general_ci".to_string(),
        is_default: true,
    });
    c.add(Charset {
        id: 33,
        name: "utf8mb3".to_string(),
        collation: "utf8mb3_general_ci".to_string(),
        is_default: true,
    });
    c.add(Charset {
        id: 34,
        name: "cp1250".to_string(),
        collation: "cp1250_czech_cs".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 36,
        name: "cp866".to_string(),
        collation: "cp866_general_ci".to_string(),
        is_default: true,
    });
    c.add(Charset {
        id: 37,
        name: "keybcs2".to_string(),
        collation: "keybcs2_general_ci".to_string(),
        is_default: true,
    });
    c.add(Charset {
        id: 38,
        name: "macce".to_string(),
        collation: "macce_general_ci".to_string(),
        is_default: true,
    });
    c.add(Charset {
        id: 39,
        name: "macroman".to_string(),
        collation: "macroman_general_ci".to_string(),
        is_default: true,
    });
    c.add(Charset {
        id: 40,
        name: "cp852".to_string(),
        collation: "cp852_general_ci".to_string(),
        is_default: true,
    });
    c.add(Charset {
        id: 41,
        name: "latin7".to_string(),
        collation: "latin7_general_ci".to_string(),
        is_default: true,
    });
    c.add(Charset {
        id: 42,
        name: "latin7".to_string(),
        collation: "latin7_general_cs".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 43,
        name: "macce".to_string(),
        collation: "macce_bin".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 44,
        name: "cp1250".to_string(),
        collation: "cp1250_croatian_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 45,
        name: "utf8mb4".to_string(),
        collation: "utf8mb4_general_ci".to_string(),
        is_default: true,
    });
    c.add(Charset {
        id: 46,
        name: "utf8mb4".to_string(),
        collation: "utf8mb4_bin".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 47,
        name: "latin1".to_string(),
        collation: "latin1_bin".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 48,
        name: "latin1".to_string(),
        collation: "latin1_general_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 49,
        name: "latin1".to_string(),
        collation: "latin1_general_cs".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 50,
        name: "cp1251".to_string(),
        collation: "cp1251_bin".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 51,
        name: "cp1251".to_string(),
        collation: "cp1251_general_ci".to_string(),
        is_default: true,
    });
    c.add(Charset {
        id: 52,
        name: "cp1251".to_string(),
        collation: "cp1251_general_cs".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 53,
        name: "macroman".to_string(),
        collation: "macroman_bin".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 57,
        name: "cp1256".to_string(),
        collation: "cp1256_general_ci".to_string(),
        is_default: true,
    });
    c.add(Charset {
        id: 58,
        name: "cp1257".to_string(),
        collation: "cp1257_bin".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 59,
        name: "cp1257".to_string(),
        collation: "cp1257_general_ci".to_string(),
        is_default: true,
    });
    c.add(Charset {
        id: 63,
        name: "binary".to_string(),
        collation: "binary".to_string(),
        is_default: true,
    });
    c.add(Charset {
        id: 64,
        name: "armscii8".to_string(),
        collation: "armscii8_bin".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 65,
        name: "ascii".to_string(),
        collation: "ascii_bin".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 66,
        name: "cp1250".to_string(),
        collation: "cp1250_bin".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 67,
        name: "cp1256".to_string(),
        collation: "cp1256_bin".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 68,
        name: "cp866".to_string(),
        collation: "cp866_bin".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 69,
        name: "dec8".to_string(),
        collation: "dec8_bin".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 70,
        name: "greek".to_string(),
        collation: "greek_bin".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 71,
        name: "hebrew".to_string(),
        collation: "hebrew_bin".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 72,
        name: "hp8".to_string(),
        collation: "hp8_bin".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 73,
        name: "keybcs2".to_string(),
        collation: "keybcs2_bin".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 74,
        name: "koi8r".to_string(),
        collation: "koi8r_bin".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 75,
        name: "koi8u".to_string(),
        collation: "koi8u_bin".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 76,
        name: "utf8mb3".to_string(),
        collation: "utf8mb3_tolower_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 77,
        name: "latin2".to_string(),
        collation: "latin2_bin".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 78,
        name: "latin5".to_string(),
        collation: "latin5_bin".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 79,
        name: "latin7".to_string(),
        collation: "latin7_bin".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 80,
        name: "cp850".to_string(),
        collation: "cp850_bin".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 81,
        name: "cp852".to_string(),
        collation: "cp852_bin".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 82,
        name: "swe7".to_string(),
        collation: "swe7_bin".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 83,
        name: "utf8mb3".to_string(),
        collation: "utf8mb3_bin".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 84,
        name: "big5".to_string(),
        collation: "big5_bin".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 85,
        name: "euckr".to_string(),
        collation: "euckr_bin".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 86,
        name: "gb2312".to_string(),
        collation: "gb2312_bin".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 87,
        name: "gbk".to_string(),
        collation: "gbk_bin".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 88,
        name: "sjis".to_string(),
        collation: "sjis_bin".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 89,
        name: "tis620".to_string(),
        collation: "tis620_bin".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 91,
        name: "ujis".to_string(),
        collation: "ujis_bin".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 92,
        name: "geostd8".to_string(),
        collation: "geostd8_general_ci".to_string(),
        is_default: true,
    });
    c.add(Charset {
        id: 93,
        name: "geostd8".to_string(),
        collation: "geostd8_bin".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 94,
        name: "latin1".to_string(),
        collation: "latin1_spanish_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 95,
        name: "cp932".to_string(),
        collation: "cp932_japanese_ci".to_string(),
        is_default: true,
    });
    c.add(Charset {
        id: 96,
        name: "cp932".to_string(),
        collation: "cp932_bin".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 97,
        name: "eucjpms".to_string(),
        collation: "eucjpms_japanese_ci".to_string(),
        is_default: true,
    });
    c.add(Charset {
        id: 98,
        name: "eucjpms".to_string(),
        collation: "eucjpms_bin".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 99,
        name: "cp1250".to_string(),
        collation: "cp1250_polish_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 192,
        name: "utf8mb3".to_string(),
        collation: "utf8mb3_unicode_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 193,
        name: "utf8mb3".to_string(),
        collation: "utf8mb3_icelandic_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 194,
        name: "utf8mb3".to_string(),
        collation: "utf8mb3_latvian_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 195,
        name: "utf8mb3".to_string(),
        collation: "utf8mb3_romanian_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 196,
        name: "utf8mb3".to_string(),
        collation: "utf8mb3_slovenian_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 197,
        name: "utf8mb3".to_string(),
        collation: "utf8mb3_polish_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 198,
        name: "utf8mb3".to_string(),
        collation: "utf8mb3_estonian_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 199,
        name: "utf8mb3".to_string(),
        collation: "utf8mb3_spanish_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 200,
        name: "utf8mb3".to_string(),
        collation: "utf8mb3_swedish_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 201,
        name: "utf8mb3".to_string(),
        collation: "utf8mb3_turkish_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 202,
        name: "utf8mb3".to_string(),
        collation: "utf8mb3_czech_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 203,
        name: "utf8mb3".to_string(),
        collation: "utf8mb3_danish_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 204,
        name: "utf8mb3".to_string(),
        collation: "utf8mb3_lithuanian_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 205,
        name: "utf8mb3".to_string(),
        collation: "utf8mb3_slovak_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 206,
        name: "utf8mb3".to_string(),
        collation: "utf8mb3_spanish2_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 207,
        name: "utf8mb3".to_string(),
        collation: "utf8mb3_roman_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 208,
        name: "utf8mb3".to_string(),
        collation: "utf8mb3_persian_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 209,
        name: "utf8mb3".to_string(),
        collation: "utf8mb3_esperanto_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 210,
        name: "utf8mb3".to_string(),
        collation: "utf8mb3_hungarian_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 211,
        name: "utf8mb3".to_string(),
        collation: "utf8mb3_sinhala_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 212,
        name: "utf8mb3".to_string(),
        collation: "utf8mb3_german2_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 213,
        name: "utf8mb3".to_string(),
        collation: "utf8mb3_croatian_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 214,
        name: "utf8mb3".to_string(),
        collation: "utf8mb3_unicode_520_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 215,
        name: "utf8mb3".to_string(),
        collation: "utf8mb3_vietnamese_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 223,
        name: "utf8mb3".to_string(),
        collation: "utf8mb3_general_mysql500_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 224,
        name: "utf8mb4".to_string(),
        collation: "utf8mb4_unicode_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 225,
        name: "utf8mb4".to_string(),
        collation: "utf8mb4_icelandic_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 226,
        name: "utf8mb4".to_string(),
        collation: "utf8mb4_latvian_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 227,
        name: "utf8mb4".to_string(),
        collation: "utf8mb4_romanian_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 228,
        name: "utf8mb4".to_string(),
        collation: "utf8mb4_slovenian_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 229,
        name: "utf8mb4".to_string(),
        collation: "utf8mb4_polish_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 230,
        name: "utf8mb4".to_string(),
        collation: "utf8mb4_estonian_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 231,
        name: "utf8mb4".to_string(),
        collation: "utf8mb4_spanish_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 232,
        name: "utf8mb4".to_string(),
        collation: "utf8mb4_swedish_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 233,
        name: "utf8mb4".to_string(),
        collation: "utf8mb4_turkish_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 234,
        name: "utf8mb4".to_string(),
        collation: "utf8mb4_czech_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 235,
        name: "utf8mb4".to_string(),
        collation: "utf8mb4_danish_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 236,
        name: "utf8mb4".to_string(),
        collation: "utf8mb4_lithuanian_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 237,
        name: "utf8mb4".to_string(),
        collation: "utf8mb4_slovak_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 238,
        name: "utf8mb4".to_string(),
        collation: "utf8mb4_spanish2_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 239,
        name: "utf8mb4".to_string(),
        collation: "utf8mb4_roman_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 240,
        name: "utf8mb4".to_string(),
        collation: "utf8mb4_persian_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 241,
        name: "utf8mb4".to_string(),
        collation: "utf8mb4_esperanto_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 242,
        name: "utf8mb4".to_string(),
        collation: "utf8mb4_hungarian_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 243,
        name: "utf8mb4".to_string(),
        collation: "utf8mb4_sinhala_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 244,
        name: "utf8mb4".to_string(),
        collation: "utf8mb4_german2_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 245,
        name: "utf8mb4".to_string(),
        collation: "utf8mb4_croatian_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 246,
        name: "utf8mb4".to_string(),
        collation: "utf8mb4_unicode_520_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 247,
        name: "utf8mb4".to_string(),
        collation: "utf8mb4_vietnamese_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 248,
        name: "gb18030".to_string(),
        collation: "gb18030_chinese_ci".to_string(),
        is_default: true,
    });
    c.add(Charset {
        id: 249,
        name: "gb18030".to_string(),
        collation: "gb18030_bin".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 250,
        name: "gb18030".to_string(),
        collation: "gb18030_unicode_520_ci".to_string(),
        is_default: false,
    });
    c.add(Charset {
        id: 255,
        name: "utf8mb4".to_string(),
        collation: "utf8mb4_0900_ai_ci".to_string(),
        is_default: false,
    });
    c
});

pub fn charset_by_id(id: u16) -> Option<&'static Charset> {
    CHARSETS.by_id(id)
}

pub fn charset_by_name(name: &str) -> Option<&'static Charset> {
    CHARSETS.by_name(name)
}

pub fn mblength(charsetnr: u16) -> usize {
    match charsetnr {
        // 4 バイト文字セット
        255 | 248 => 4,
        // 3 バイト文字セット (eucjpms, ujis)
        97 | 98 | 91 => 3,
        // 2 バイト文字セット (gbk, gb2312, big5, euckr, cp932, sjis)
        28 | 87 | 24 | 86 | 1 | 19 | 85 | 95 | 96 | 13 => 2,
        // それ以外はシングルバイト
        _ => 1,
    }
}
