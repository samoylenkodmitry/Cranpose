//! ETC1S grayscale to BC7 mode 5 alpha conversion table.

/// One conversion entry: the two BC7 mode 5 alpha endpoints (`m_lo`, `m_hi`)
/// and `m_trans`, the per-source-selector remap packed two bits per selector.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Etc1GToBc7M5aConversion {
    pub m_lo: u8,
    pub m_hi: u8,
    pub m_trans: u8,
}

/// Indexed by `(inten * 32 + base_red) * NUM_SELECTOR_RANGES + selector_range`:
/// the best BC7 mode 5 alpha encoding of an ETC1S grayscale endpoint and
/// selector range.
pub static G_ETC1_G_TO_BC7_M5A: [Etc1GToBc7M5aConversion; 1536] = [
    Etc1GToBc7M5aConversion {
        m_lo: 7,
        m_hi: 0,
        m_trans: 47,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 7,
        m_hi: 0,
        m_trans: 44,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 2,
        m_hi: 0,
        m_trans: 15,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 2,
        m_hi: 0,
        m_trans: 12,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 8,
        m_hi: 2,
        m_trans: 48,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 0,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 15,
        m_hi: 0,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 15,
        m_hi: 1,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 9,
        m_hi: 0,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 15,
        m_hi: 1,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 23,
        m_hi: 3,
        m_trans: 96,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 6,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 24,
        m_hi: 7,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 24,
        m_hi: 7,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 18,
        m_hi: 3,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 18,
        m_hi: 5,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 24,
        m_hi: 5,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 21,
        m_hi: 1,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 32,
        m_hi: 15,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 32,
        m_hi: 15,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 26,
        m_hi: 11,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 26,
        m_hi: 13,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 32,
        m_hi: 13,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 22,
        m_hi: 3,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 41,
        m_hi: 24,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 41,
        m_hi: 24,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 35,
        m_hi: 20,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 35,
        m_hi: 22,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 41,
        m_hi: 22,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 31,
        m_hi: 12,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 49,
        m_hi: 32,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 49,
        m_hi: 32,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 43,
        m_hi: 28,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 43,
        m_hi: 30,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 49,
        m_hi: 30,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 39,
        m_hi: 20,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 57,
        m_hi: 40,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 57,
        m_hi: 40,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 51,
        m_hi: 36,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 51,
        m_hi: 38,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 57,
        m_hi: 38,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 47,
        m_hi: 28,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 65,
        m_hi: 48,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 65,
        m_hi: 48,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 59,
        m_hi: 44,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 59,
        m_hi: 46,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 65,
        m_hi: 46,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 55,
        m_hi: 36,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 74,
        m_hi: 57,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 74,
        m_hi: 57,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 68,
        m_hi: 53,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 68,
        m_hi: 55,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 74,
        m_hi: 55,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 64,
        m_hi: 45,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 82,
        m_hi: 65,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 82,
        m_hi: 65,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 76,
        m_hi: 61,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 76,
        m_hi: 63,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 82,
        m_hi: 63,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 72,
        m_hi: 53,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 90,
        m_hi: 73,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 90,
        m_hi: 73,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 84,
        m_hi: 69,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 84,
        m_hi: 71,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 90,
        m_hi: 71,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 80,
        m_hi: 61,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 98,
        m_hi: 81,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 98,
        m_hi: 81,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 92,
        m_hi: 77,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 92,
        m_hi: 79,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 98,
        m_hi: 79,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 88,
        m_hi: 69,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 107,
        m_hi: 90,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 107,
        m_hi: 90,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 101,
        m_hi: 86,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 101,
        m_hi: 88,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 107,
        m_hi: 88,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 97,
        m_hi: 78,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 115,
        m_hi: 98,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 115,
        m_hi: 98,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 109,
        m_hi: 94,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 109,
        m_hi: 96,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 115,
        m_hi: 96,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 105,
        m_hi: 86,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 123,
        m_hi: 106,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 123,
        m_hi: 106,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 117,
        m_hi: 102,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 117,
        m_hi: 104,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 123,
        m_hi: 104,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 113,
        m_hi: 94,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 131,
        m_hi: 114,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 131,
        m_hi: 114,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 125,
        m_hi: 110,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 125,
        m_hi: 112,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 131,
        m_hi: 112,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 121,
        m_hi: 102,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 140,
        m_hi: 123,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 140,
        m_hi: 123,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 134,
        m_hi: 119,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 134,
        m_hi: 121,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 140,
        m_hi: 121,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 130,
        m_hi: 111,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 148,
        m_hi: 131,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 148,
        m_hi: 131,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 142,
        m_hi: 127,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 142,
        m_hi: 129,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 148,
        m_hi: 129,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 138,
        m_hi: 119,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 156,
        m_hi: 139,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 156,
        m_hi: 139,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 150,
        m_hi: 135,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 150,
        m_hi: 137,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 156,
        m_hi: 137,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 146,
        m_hi: 127,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 164,
        m_hi: 147,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 164,
        m_hi: 147,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 158,
        m_hi: 143,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 158,
        m_hi: 145,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 164,
        m_hi: 145,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 154,
        m_hi: 135,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 173,
        m_hi: 156,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 173,
        m_hi: 156,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 167,
        m_hi: 152,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 167,
        m_hi: 154,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 173,
        m_hi: 154,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 163,
        m_hi: 144,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 181,
        m_hi: 164,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 181,
        m_hi: 164,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 175,
        m_hi: 160,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 175,
        m_hi: 162,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 181,
        m_hi: 162,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 171,
        m_hi: 152,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 189,
        m_hi: 172,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 189,
        m_hi: 172,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 183,
        m_hi: 168,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 183,
        m_hi: 170,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 189,
        m_hi: 170,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 179,
        m_hi: 160,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 197,
        m_hi: 180,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 197,
        m_hi: 180,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 191,
        m_hi: 176,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 191,
        m_hi: 178,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 197,
        m_hi: 178,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 187,
        m_hi: 168,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 206,
        m_hi: 189,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 206,
        m_hi: 189,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 200,
        m_hi: 185,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 200,
        m_hi: 187,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 206,
        m_hi: 187,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 196,
        m_hi: 177,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 214,
        m_hi: 197,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 214,
        m_hi: 197,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 208,
        m_hi: 193,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 208,
        m_hi: 195,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 214,
        m_hi: 195,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 204,
        m_hi: 185,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 222,
        m_hi: 205,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 222,
        m_hi: 205,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 216,
        m_hi: 201,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 216,
        m_hi: 203,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 222,
        m_hi: 203,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 212,
        m_hi: 193,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 230,
        m_hi: 213,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 230,
        m_hi: 213,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 224,
        m_hi: 209,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 224,
        m_hi: 211,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 230,
        m_hi: 211,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 220,
        m_hi: 201,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 239,
        m_hi: 222,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 239,
        m_hi: 222,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 233,
        m_hi: 218,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 233,
        m_hi: 220,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 239,
        m_hi: 220,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 229,
        m_hi: 210,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 247,
        m_hi: 230,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 247,
        m_hi: 230,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 241,
        m_hi: 226,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 241,
        m_hi: 228,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 247,
        m_hi: 228,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 237,
        m_hi: 218,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 238,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 238,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 249,
        m_hi: 234,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 249,
        m_hi: 236,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 236,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 245,
        m_hi: 226,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 247,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 248,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 247,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 248,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 253,
        m_hi: 234,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 16,
        m_hi: 0,
        m_trans: 47,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 16,
        m_hi: 0,
        m_trans: 44,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 5,
        m_hi: 0,
        m_trans: 15,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 5,
        m_hi: 0,
        m_trans: 12,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 17,
        m_hi: 5,
        m_trans: 48,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 0,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 37,
        m_hi: 1,
        m_trans: 111,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 36,
        m_hi: 2,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 13,
        m_hi: 0,
        m_trans: 11,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 13,
        m_hi: 3,
        m_trans: 12,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 37,
        m_hi: 1,
        m_trans: 96,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 3,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 33,
        m_hi: 0,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 33,
        m_hi: 0,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 31,
        m_hi: 0,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 31,
        m_hi: 1,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 45,
        m_hi: 9,
        m_trans: 96,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 11,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 40,
        m_hi: 7,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 41,
        m_hi: 8,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 39,
        m_hi: 7,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 29,
        m_hi: 0,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 41,
        m_hi: 3,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 19,
        m_hi: 1,
        m_trans: 2,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 49,
        m_hi: 16,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 50,
        m_hi: 17,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 38,
        m_hi: 5,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 38,
        m_hi: 6,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 50,
        m_hi: 12,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 40,
        m_hi: 4,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 57,
        m_hi: 24,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 58,
        m_hi: 25,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 46,
        m_hi: 13,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 46,
        m_hi: 14,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 58,
        m_hi: 20,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 36,
        m_hi: 0,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 65,
        m_hi: 32,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 66,
        m_hi: 33,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 54,
        m_hi: 21,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 54,
        m_hi: 22,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 66,
        m_hi: 28,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 44,
        m_hi: 6,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 73,
        m_hi: 40,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 74,
        m_hi: 41,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 62,
        m_hi: 29,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 62,
        m_hi: 30,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 74,
        m_hi: 36,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 52,
        m_hi: 14,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 82,
        m_hi: 49,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 83,
        m_hi: 50,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 71,
        m_hi: 38,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 71,
        m_hi: 39,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 83,
        m_hi: 45,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 61,
        m_hi: 23,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 90,
        m_hi: 57,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 91,
        m_hi: 58,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 79,
        m_hi: 46,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 79,
        m_hi: 47,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 91,
        m_hi: 53,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 69,
        m_hi: 31,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 98,
        m_hi: 65,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 99,
        m_hi: 66,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 87,
        m_hi: 54,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 87,
        m_hi: 55,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 99,
        m_hi: 61,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 77,
        m_hi: 39,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 106,
        m_hi: 73,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 107,
        m_hi: 74,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 95,
        m_hi: 62,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 95,
        m_hi: 63,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 107,
        m_hi: 69,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 85,
        m_hi: 47,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 115,
        m_hi: 82,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 116,
        m_hi: 83,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 104,
        m_hi: 71,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 104,
        m_hi: 72,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 116,
        m_hi: 78,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 94,
        m_hi: 56,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 123,
        m_hi: 90,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 124,
        m_hi: 91,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 112,
        m_hi: 79,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 112,
        m_hi: 80,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 124,
        m_hi: 86,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 102,
        m_hi: 64,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 131,
        m_hi: 98,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 132,
        m_hi: 99,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 120,
        m_hi: 87,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 120,
        m_hi: 88,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 132,
        m_hi: 94,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 110,
        m_hi: 72,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 139,
        m_hi: 106,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 140,
        m_hi: 107,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 128,
        m_hi: 95,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 128,
        m_hi: 96,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 140,
        m_hi: 102,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 118,
        m_hi: 80,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 148,
        m_hi: 115,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 149,
        m_hi: 116,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 137,
        m_hi: 104,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 137,
        m_hi: 105,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 149,
        m_hi: 111,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 127,
        m_hi: 89,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 156,
        m_hi: 123,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 157,
        m_hi: 124,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 145,
        m_hi: 112,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 145,
        m_hi: 113,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 157,
        m_hi: 119,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 135,
        m_hi: 97,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 164,
        m_hi: 131,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 165,
        m_hi: 132,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 153,
        m_hi: 120,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 153,
        m_hi: 121,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 165,
        m_hi: 127,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 143,
        m_hi: 105,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 172,
        m_hi: 139,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 173,
        m_hi: 140,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 161,
        m_hi: 128,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 161,
        m_hi: 129,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 173,
        m_hi: 135,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 151,
        m_hi: 113,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 181,
        m_hi: 148,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 182,
        m_hi: 149,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 170,
        m_hi: 137,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 170,
        m_hi: 138,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 182,
        m_hi: 144,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 160,
        m_hi: 122,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 189,
        m_hi: 156,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 190,
        m_hi: 157,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 178,
        m_hi: 145,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 178,
        m_hi: 146,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 190,
        m_hi: 152,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 168,
        m_hi: 130,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 197,
        m_hi: 164,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 198,
        m_hi: 165,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 186,
        m_hi: 153,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 186,
        m_hi: 154,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 198,
        m_hi: 160,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 176,
        m_hi: 138,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 205,
        m_hi: 172,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 206,
        m_hi: 173,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 194,
        m_hi: 161,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 194,
        m_hi: 162,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 206,
        m_hi: 168,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 184,
        m_hi: 146,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 214,
        m_hi: 181,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 215,
        m_hi: 182,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 203,
        m_hi: 170,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 203,
        m_hi: 171,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 215,
        m_hi: 177,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 193,
        m_hi: 155,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 222,
        m_hi: 189,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 223,
        m_hi: 190,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 211,
        m_hi: 178,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 211,
        m_hi: 179,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 223,
        m_hi: 185,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 201,
        m_hi: 163,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 230,
        m_hi: 197,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 231,
        m_hi: 198,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 219,
        m_hi: 186,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 219,
        m_hi: 187,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 231,
        m_hi: 193,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 209,
        m_hi: 171,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 238,
        m_hi: 205,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 239,
        m_hi: 206,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 227,
        m_hi: 194,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 227,
        m_hi: 195,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 239,
        m_hi: 201,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 217,
        m_hi: 179,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 247,
        m_hi: 214,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 248,
        m_hi: 215,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 236,
        m_hi: 203,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 236,
        m_hi: 204,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 248,
        m_hi: 210,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 226,
        m_hi: 188,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 222,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 222,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 244,
        m_hi: 211,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 244,
        m_hi: 212,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 220,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 234,
        m_hi: 196,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 254,
        m_hi: 218,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 242,
        m_trans: 28,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 252,
        m_hi: 219,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 252,
        m_hi: 220,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 245,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 242,
        m_hi: 204,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 238,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 239,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 238,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 239,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 250,
        m_hi: 212,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 28,
        m_hi: 0,
        m_trans: 47,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 28,
        m_hi: 0,
        m_trans: 44,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 9,
        m_hi: 0,
        m_trans: 15,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 9,
        m_hi: 0,
        m_trans: 12,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 29,
        m_hi: 9,
        m_trans: 48,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 0,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 53,
        m_hi: 0,
        m_trans: 111,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 53,
        m_hi: 0,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 17,
        m_hi: 0,
        m_trans: 15,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 17,
        m_hi: 0,
        m_trans: 12,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 37,
        m_hi: 7,
        m_trans: 32,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 0,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 66,
        m_hi: 3,
        m_trans: 111,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 63,
        m_hi: 7,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 25,
        m_hi: 0,
        m_trans: 11,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 25,
        m_hi: 7,
        m_trans: 12,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 65,
        m_hi: 5,
        m_trans: 96,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 7,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 51,
        m_hi: 0,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 71,
        m_hi: 15,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 47,
        m_hi: 0,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 33,
        m_hi: 6,
        m_trans: 8,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 73,
        m_hi: 13,
        m_trans: 96,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 15,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 61,
        m_hi: 4,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 80,
        m_hi: 24,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 61,
        m_hi: 4,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 60,
        m_hi: 6,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 62,
        m_hi: 0,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 24,
        m_hi: 4,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 69,
        m_hi: 12,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 88,
        m_hi: 32,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 69,
        m_hi: 12,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 68,
        m_hi: 14,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 70,
        m_hi: 8,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 32,
        m_hi: 2,
        m_trans: 2,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 77,
        m_hi: 20,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 96,
        m_hi: 40,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 58,
        m_hi: 2,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 58,
        m_hi: 2,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 78,
        m_hi: 16,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 60,
        m_hi: 0,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 85,
        m_hi: 28,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 104,
        m_hi: 48,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 66,
        m_hi: 10,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 66,
        m_hi: 10,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 86,
        m_hi: 24,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 68,
        m_hi: 8,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 94,
        m_hi: 37,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 113,
        m_hi: 57,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 75,
        m_hi: 19,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 75,
        m_hi: 19,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 95,
        m_hi: 33,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 77,
        m_hi: 17,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 102,
        m_hi: 45,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 121,
        m_hi: 65,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 83,
        m_hi: 27,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 83,
        m_hi: 27,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 103,
        m_hi: 41,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 65,
        m_hi: 3,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 110,
        m_hi: 53,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 129,
        m_hi: 73,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 91,
        m_hi: 35,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 91,
        m_hi: 35,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 111,
        m_hi: 49,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 73,
        m_hi: 11,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 118,
        m_hi: 61,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 137,
        m_hi: 81,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 99,
        m_hi: 43,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 99,
        m_hi: 43,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 119,
        m_hi: 57,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 81,
        m_hi: 19,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 127,
        m_hi: 70,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 146,
        m_hi: 90,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 108,
        m_hi: 52,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 108,
        m_hi: 52,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 128,
        m_hi: 66,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 90,
        m_hi: 28,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 135,
        m_hi: 78,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 154,
        m_hi: 98,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 116,
        m_hi: 60,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 116,
        m_hi: 60,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 136,
        m_hi: 74,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 98,
        m_hi: 36,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 143,
        m_hi: 86,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 162,
        m_hi: 106,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 124,
        m_hi: 68,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 124,
        m_hi: 68,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 144,
        m_hi: 82,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 106,
        m_hi: 44,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 151,
        m_hi: 94,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 170,
        m_hi: 114,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 132,
        m_hi: 76,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 132,
        m_hi: 76,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 152,
        m_hi: 90,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 114,
        m_hi: 52,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 160,
        m_hi: 103,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 179,
        m_hi: 123,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 141,
        m_hi: 85,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 141,
        m_hi: 85,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 161,
        m_hi: 99,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 123,
        m_hi: 61,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 168,
        m_hi: 111,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 187,
        m_hi: 131,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 149,
        m_hi: 93,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 149,
        m_hi: 93,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 169,
        m_hi: 107,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 131,
        m_hi: 69,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 176,
        m_hi: 119,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 195,
        m_hi: 139,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 157,
        m_hi: 101,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 157,
        m_hi: 101,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 177,
        m_hi: 115,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 139,
        m_hi: 77,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 184,
        m_hi: 127,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 203,
        m_hi: 147,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 165,
        m_hi: 109,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 165,
        m_hi: 109,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 185,
        m_hi: 123,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 147,
        m_hi: 85,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 193,
        m_hi: 136,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 212,
        m_hi: 156,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 174,
        m_hi: 118,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 174,
        m_hi: 118,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 194,
        m_hi: 132,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 156,
        m_hi: 94,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 201,
        m_hi: 144,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 220,
        m_hi: 164,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 182,
        m_hi: 126,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 182,
        m_hi: 126,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 202,
        m_hi: 140,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 164,
        m_hi: 102,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 209,
        m_hi: 152,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 228,
        m_hi: 172,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 190,
        m_hi: 134,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 190,
        m_hi: 134,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 210,
        m_hi: 148,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 172,
        m_hi: 110,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 217,
        m_hi: 160,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 236,
        m_hi: 180,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 198,
        m_hi: 142,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 198,
        m_hi: 142,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 218,
        m_hi: 156,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 180,
        m_hi: 118,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 226,
        m_hi: 169,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 245,
        m_hi: 189,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 207,
        m_hi: 151,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 207,
        m_hi: 151,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 227,
        m_hi: 165,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 189,
        m_hi: 127,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 234,
        m_hi: 177,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 253,
        m_hi: 197,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 215,
        m_hi: 159,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 215,
        m_hi: 159,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 235,
        m_hi: 173,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 197,
        m_hi: 135,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 242,
        m_hi: 185,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 243,
        m_hi: 186,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 223,
        m_hi: 167,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 223,
        m_hi: 167,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 243,
        m_hi: 181,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 205,
        m_hi: 143,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 250,
        m_hi: 193,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 251,
        m_hi: 194,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 231,
        m_hi: 175,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 231,
        m_hi: 175,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 251,
        m_hi: 189,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 213,
        m_hi: 151,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 203,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 206,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 240,
        m_hi: 184,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 240,
        m_hi: 184,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 208,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 222,
        m_hi: 160,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 251,
        m_hi: 188,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 230,
        m_trans: 28,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 248,
        m_hi: 192,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 248,
        m_hi: 192,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 233,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 230,
        m_hi: 168,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 200,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 202,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 200,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 202,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 238,
        m_hi: 176,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 226,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 227,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 226,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 227,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 246,
        m_hi: 184,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 41,
        m_hi: 0,
        m_trans: 47,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 41,
        m_hi: 0,
        m_trans: 44,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 13,
        m_hi: 0,
        m_trans: 15,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 13,
        m_hi: 0,
        m_trans: 12,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 42,
        m_hi: 13,
        m_trans: 48,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 0,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 71,
        m_hi: 0,
        m_trans: 111,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 71,
        m_hi: 0,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 21,
        m_hi: 0,
        m_trans: 15,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 21,
        m_hi: 0,
        m_trans: 12,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 50,
        m_hi: 7,
        m_trans: 32,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 0,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 86,
        m_hi: 1,
        m_trans: 111,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 85,
        m_hi: 2,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 29,
        m_hi: 1,
        m_trans: 15,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 29,
        m_hi: 3,
        m_trans: 12,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 87,
        m_hi: 0,
        m_trans: 96,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 3,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 96,
        m_hi: 6,
        m_trans: 111,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 93,
        m_hi: 10,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 37,
        m_hi: 0,
        m_trans: 11,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 37,
        m_hi: 11,
        m_trans: 12,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 95,
        m_hi: 8,
        m_trans: 96,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 11,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 71,
        m_hi: 0,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 102,
        m_hi: 19,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 68,
        m_hi: 0,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 46,
        m_hi: 7,
        m_trans: 8,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 104,
        m_hi: 17,
        m_trans: 96,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 20,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 82,
        m_hi: 0,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 110,
        m_hi: 27,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 81,
        m_hi: 0,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 80,
        m_hi: 2,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 112,
        m_hi: 25,
        m_trans: 96,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 28,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 91,
        m_hi: 7,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 118,
        m_hi: 35,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 91,
        m_hi: 7,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 88,
        m_hi: 10,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 91,
        m_hi: 2,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 36,
        m_hi: 7,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 99,
        m_hi: 15,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 126,
        m_hi: 43,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 99,
        m_hi: 15,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 96,
        m_hi: 18,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 99,
        m_hi: 10,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 44,
        m_hi: 1,
        m_trans: 2,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 108,
        m_hi: 24,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 135,
        m_hi: 52,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 108,
        m_hi: 24,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 79,
        m_hi: 0,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 108,
        m_hi: 19,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 53,
        m_hi: 10,
        m_trans: 2,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 116,
        m_hi: 32,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 143,
        m_hi: 60,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 87,
        m_hi: 5,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 87,
        m_hi: 7,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 116,
        m_hi: 27,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 90,
        m_hi: 3,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 124,
        m_hi: 40,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 151,
        m_hi: 68,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 95,
        m_hi: 13,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 95,
        m_hi: 15,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 124,
        m_hi: 35,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 98,
        m_hi: 11,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 132,
        m_hi: 48,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 159,
        m_hi: 76,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 103,
        m_hi: 21,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 103,
        m_hi: 23,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 132,
        m_hi: 43,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 106,
        m_hi: 19,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 141,
        m_hi: 57,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 168,
        m_hi: 85,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 112,
        m_hi: 30,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 112,
        m_hi: 32,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 141,
        m_hi: 52,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 115,
        m_hi: 28,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 149,
        m_hi: 65,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 176,
        m_hi: 93,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 120,
        m_hi: 38,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 120,
        m_hi: 40,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 149,
        m_hi: 60,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 94,
        m_hi: 5,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 157,
        m_hi: 73,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 184,
        m_hi: 101,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 128,
        m_hi: 46,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 128,
        m_hi: 48,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 157,
        m_hi: 68,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 102,
        m_hi: 13,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 165,
        m_hi: 81,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 192,
        m_hi: 109,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 136,
        m_hi: 54,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 136,
        m_hi: 56,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 165,
        m_hi: 76,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 110,
        m_hi: 21,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 174,
        m_hi: 90,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 201,
        m_hi: 118,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 145,
        m_hi: 63,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 145,
        m_hi: 65,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 174,
        m_hi: 85,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 119,
        m_hi: 30,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 182,
        m_hi: 98,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 209,
        m_hi: 126,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 153,
        m_hi: 71,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 153,
        m_hi: 73,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 182,
        m_hi: 93,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 127,
        m_hi: 38,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 190,
        m_hi: 106,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 217,
        m_hi: 134,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 161,
        m_hi: 79,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 161,
        m_hi: 81,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 190,
        m_hi: 101,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 135,
        m_hi: 46,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 198,
        m_hi: 114,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 225,
        m_hi: 142,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 169,
        m_hi: 87,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 169,
        m_hi: 89,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 198,
        m_hi: 109,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 143,
        m_hi: 54,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 207,
        m_hi: 123,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 234,
        m_hi: 151,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 178,
        m_hi: 96,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 178,
        m_hi: 98,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 207,
        m_hi: 118,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 152,
        m_hi: 63,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 215,
        m_hi: 131,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 242,
        m_hi: 159,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 186,
        m_hi: 104,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 186,
        m_hi: 106,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 215,
        m_hi: 126,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 160,
        m_hi: 71,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 223,
        m_hi: 139,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 250,
        m_hi: 167,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 194,
        m_hi: 112,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 194,
        m_hi: 114,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 223,
        m_hi: 134,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 168,
        m_hi: 79,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 231,
        m_hi: 147,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 231,
        m_hi: 147,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 202,
        m_hi: 120,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 202,
        m_hi: 122,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 231,
        m_hi: 142,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 176,
        m_hi: 87,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 240,
        m_hi: 156,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 240,
        m_hi: 156,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 211,
        m_hi: 129,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 211,
        m_hi: 131,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 240,
        m_hi: 151,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 185,
        m_hi: 96,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 248,
        m_hi: 164,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 248,
        m_hi: 164,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 219,
        m_hi: 137,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 219,
        m_hi: 139,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 248,
        m_hi: 159,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 193,
        m_hi: 104,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 254,
        m_hi: 173,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 174,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 227,
        m_hi: 145,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 227,
        m_hi: 147,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 169,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 201,
        m_hi: 112,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 182,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 187,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 235,
        m_hi: 153,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 235,
        m_hi: 155,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 193,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 209,
        m_hi: 120,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 249,
        m_hi: 159,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 218,
        m_trans: 28,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 244,
        m_hi: 162,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 244,
        m_hi: 164,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 220,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 218,
        m_hi: 129,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 254,
        m_hi: 169,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 254,
        m_hi: 168,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 252,
        m_hi: 170,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 252,
        m_hi: 172,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 245,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 226,
        m_hi: 137,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 184,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 190,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 184,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 190,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 234,
        m_hi: 145,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 213,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 214,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 213,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 214,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 242,
        m_hi: 153,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 59,
        m_hi: 0,
        m_trans: 47,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 59,
        m_hi: 0,
        m_trans: 44,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 18,
        m_hi: 0,
        m_trans: 15,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 18,
        m_hi: 0,
        m_trans: 12,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 60,
        m_hi: 18,
        m_trans: 48,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 0,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 69,
        m_hi: 0,
        m_trans: 47,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 68,
        m_hi: 2,
        m_trans: 44,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 26,
        m_hi: 0,
        m_trans: 15,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 26,
        m_hi: 0,
        m_trans: 12,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 68,
        m_hi: 5,
        m_trans: 32,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 0,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 111,
        m_hi: 0,
        m_trans: 111,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 111,
        m_hi: 0,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 34,
        m_hi: 0,
        m_trans: 15,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 34,
        m_hi: 0,
        m_trans: 12,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 76,
        m_hi: 13,
        m_trans: 32,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 0,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 123,
        m_hi: 3,
        m_trans: 111,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 122,
        m_hi: 5,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 42,
        m_hi: 3,
        m_trans: 15,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 42,
        m_hi: 6,
        m_trans: 12,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 125,
        m_hi: 1,
        m_trans: 96,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 6,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 135,
        m_hi: 8,
        m_trans: 111,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 131,
        m_hi: 14,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 50,
        m_hi: 0,
        m_trans: 11,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 51,
        m_hi: 15,
        m_trans: 12,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 134,
        m_hi: 10,
        m_trans: 96,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 15,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 95,
        m_hi: 0,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 139,
        m_hi: 22,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 59,
        m_hi: 2,
        m_trans: 11,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 59,
        m_hi: 5,
        m_trans: 8,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 142,
        m_hi: 18,
        m_trans: 96,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 23,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 105,
        m_hi: 0,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 147,
        m_hi: 30,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 99,
        m_hi: 0,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 67,
        m_hi: 13,
        m_trans: 8,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 150,
        m_hi: 26,
        m_trans: 96,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 31,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 115,
        m_hi: 0,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 155,
        m_hi: 38,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 113,
        m_hi: 1,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 110,
        m_hi: 4,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 158,
        m_hi: 34,
        m_trans: 96,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 39,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 125,
        m_hi: 7,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 164,
        m_hi: 47,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 125,
        m_hi: 7,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 119,
        m_hi: 13,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 167,
        m_hi: 43,
        m_trans: 96,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 48,
        m_hi: 6,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 133,
        m_hi: 15,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 172,
        m_hi: 55,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 133,
        m_hi: 15,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 127,
        m_hi: 21,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 134,
        m_hi: 5,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 56,
        m_hi: 14,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 141,
        m_hi: 23,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 180,
        m_hi: 63,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 141,
        m_hi: 23,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 135,
        m_hi: 29,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 142,
        m_hi: 13,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 64,
        m_hi: 1,
        m_trans: 2,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 149,
        m_hi: 31,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 188,
        m_hi: 71,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 149,
        m_hi: 31,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 143,
        m_hi: 37,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 150,
        m_hi: 21,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 72,
        m_hi: 9,
        m_trans: 2,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 158,
        m_hi: 40,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 197,
        m_hi: 80,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 118,
        m_hi: 1,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 117,
        m_hi: 6,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 159,
        m_hi: 30,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 120,
        m_hi: 0,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 166,
        m_hi: 48,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 205,
        m_hi: 88,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 126,
        m_hi: 9,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 125,
        m_hi: 14,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 167,
        m_hi: 38,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 130,
        m_hi: 6,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 174,
        m_hi: 56,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 213,
        m_hi: 96,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 134,
        m_hi: 17,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 133,
        m_hi: 22,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 175,
        m_hi: 46,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 138,
        m_hi: 14,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 182,
        m_hi: 64,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 221,
        m_hi: 104,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 142,
        m_hi: 25,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 141,
        m_hi: 30,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 183,
        m_hi: 54,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 146,
        m_hi: 22,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 191,
        m_hi: 73,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 230,
        m_hi: 113,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 151,
        m_hi: 34,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 150,
        m_hi: 39,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 192,
        m_hi: 63,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 155,
        m_hi: 31,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 199,
        m_hi: 81,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 238,
        m_hi: 121,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 159,
        m_hi: 42,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 158,
        m_hi: 47,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 200,
        m_hi: 71,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 163,
        m_hi: 39,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 207,
        m_hi: 89,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 246,
        m_hi: 129,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 167,
        m_hi: 50,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 166,
        m_hi: 55,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 208,
        m_hi: 79,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 130,
        m_hi: 1,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 215,
        m_hi: 97,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 254,
        m_hi: 137,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 175,
        m_hi: 58,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 174,
        m_hi: 63,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 216,
        m_hi: 87,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 138,
        m_hi: 9,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 224,
        m_hi: 106,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 224,
        m_hi: 106,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 184,
        m_hi: 67,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 183,
        m_hi: 72,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 225,
        m_hi: 96,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 147,
        m_hi: 18,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 232,
        m_hi: 114,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 232,
        m_hi: 114,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 192,
        m_hi: 75,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 191,
        m_hi: 80,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 233,
        m_hi: 104,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 155,
        m_hi: 26,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 240,
        m_hi: 122,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 240,
        m_hi: 122,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 200,
        m_hi: 83,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 199,
        m_hi: 88,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 241,
        m_hi: 112,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 163,
        m_hi: 34,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 248,
        m_hi: 130,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 248,
        m_hi: 130,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 208,
        m_hi: 91,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 207,
        m_hi: 96,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 249,
        m_hi: 120,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 171,
        m_hi: 42,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 140,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 254,
        m_hi: 142,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 217,
        m_hi: 100,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 216,
        m_hi: 105,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 135,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 180,
        m_hi: 51,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 149,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 156,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 225,
        m_hi: 108,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 224,
        m_hi: 113,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 159,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 188,
        m_hi: 59,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 159,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 253,
        m_hi: 196,
        m_trans: 28,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 233,
        m_hi: 116,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 232,
        m_hi: 121,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 184,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 196,
        m_hi: 67,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 247,
        m_hi: 120,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 205,
        m_trans: 28,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 241,
        m_hi: 124,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 240,
        m_hi: 129,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 208,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 204,
        m_hi: 75,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 252,
        m_hi: 132,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 252,
        m_hi: 132,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 250,
        m_hi: 133,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 249,
        m_hi: 138,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 236,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 213,
        m_hi: 84,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 144,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 150,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 144,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 150,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 221,
        m_hi: 92,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 254,
        m_hi: 185,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 175,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 253,
        m_hi: 187,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 175,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 229,
        m_hi: 100,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 196,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 199,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 196,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 199,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 237,
        m_hi: 108,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 80,
        m_hi: 0,
        m_trans: 47,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 80,
        m_hi: 0,
        m_trans: 44,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 24,
        m_hi: 0,
        m_trans: 15,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 24,
        m_hi: 0,
        m_trans: 12,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 80,
        m_hi: 24,
        m_trans: 48,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 0,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 88,
        m_hi: 1,
        m_trans: 47,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 88,
        m_hi: 1,
        m_trans: 44,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 32,
        m_hi: 0,
        m_trans: 15,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 32,
        m_hi: 0,
        m_trans: 12,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 88,
        m_hi: 4,
        m_trans: 32,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 0,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 138,
        m_hi: 0,
        m_trans: 111,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 138,
        m_hi: 0,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 40,
        m_hi: 0,
        m_trans: 15,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 40,
        m_hi: 0,
        m_trans: 12,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 96,
        m_hi: 12,
        m_trans: 32,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 0,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 153,
        m_hi: 0,
        m_trans: 111,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 153,
        m_hi: 0,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 48,
        m_hi: 0,
        m_trans: 15,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 48,
        m_hi: 0,
        m_trans: 12,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 104,
        m_hi: 20,
        m_trans: 32,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 0,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 166,
        m_hi: 4,
        m_trans: 111,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 164,
        m_hi: 8,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 57,
        m_hi: 4,
        m_trans: 15,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 57,
        m_hi: 9,
        m_trans: 12,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 167,
        m_hi: 3,
        m_trans: 96,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 9,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 176,
        m_hi: 9,
        m_trans: 111,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 172,
        m_hi: 16,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 65,
        m_hi: 0,
        m_trans: 11,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 65,
        m_hi: 17,
        m_trans: 12,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 175,
        m_hi: 11,
        m_trans: 96,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 17,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 189,
        m_hi: 13,
        m_trans: 111,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 180,
        m_hi: 24,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 73,
        m_hi: 0,
        m_trans: 11,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 73,
        m_hi: 1,
        m_trans: 8,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 183,
        m_hi: 19,
        m_trans: 96,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 25,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 129,
        m_hi: 0,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 188,
        m_hi: 32,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 83,
        m_hi: 2,
        m_trans: 11,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 81,
        m_hi: 9,
        m_trans: 8,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 191,
        m_hi: 27,
        m_trans: 96,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 33,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 141,
        m_hi: 0,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 197,
        m_hi: 41,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 132,
        m_hi: 0,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 90,
        m_hi: 18,
        m_trans: 8,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 200,
        m_hi: 36,
        m_trans: 96,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 42,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 151,
        m_hi: 0,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 205,
        m_hi: 49,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 148,
        m_hi: 0,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 145,
        m_hi: 3,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 208,
        m_hi: 44,
        m_trans: 96,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 50,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 160,
        m_hi: 3,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 213,
        m_hi: 57,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 160,
        m_hi: 3,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 153,
        m_hi: 11,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 216,
        m_hi: 52,
        m_trans: 96,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 58,
        m_hi: 2,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 168,
        m_hi: 11,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 221,
        m_hi: 65,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 168,
        m_hi: 11,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 161,
        m_hi: 19,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 170,
        m_hi: 0,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 66,
        m_hi: 10,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 177,
        m_hi: 20,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 230,
        m_hi: 74,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 177,
        m_hi: 20,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 170,
        m_hi: 28,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 179,
        m_hi: 7,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 75,
        m_hi: 19,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 185,
        m_hi: 28,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 238,
        m_hi: 82,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 185,
        m_hi: 28,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 178,
        m_hi: 36,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 187,
        m_hi: 15,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 83,
        m_hi: 0,
        m_trans: 2,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 193,
        m_hi: 36,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 246,
        m_hi: 90,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 193,
        m_hi: 36,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 186,
        m_hi: 44,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 195,
        m_hi: 23,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 91,
        m_hi: 7,
        m_trans: 2,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 201,
        m_hi: 44,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 254,
        m_hi: 98,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 201,
        m_hi: 44,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 147,
        m_hi: 0,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 203,
        m_hi: 31,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 99,
        m_hi: 15,
        m_trans: 2,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 210,
        m_hi: 53,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 211,
        m_hi: 54,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 157,
        m_hi: 1,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 156,
        m_hi: 9,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 212,
        m_hi: 40,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 108,
        m_hi: 24,
        m_trans: 2,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 218,
        m_hi: 61,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 219,
        m_hi: 62,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 165,
        m_hi: 9,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 164,
        m_hi: 17,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 220,
        m_hi: 48,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 170,
        m_hi: 6,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 226,
        m_hi: 69,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 227,
        m_hi: 70,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 173,
        m_hi: 17,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 172,
        m_hi: 25,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 228,
        m_hi: 56,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 178,
        m_hi: 14,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 234,
        m_hi: 77,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 235,
        m_hi: 78,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 181,
        m_hi: 25,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 180,
        m_hi: 33,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 236,
        m_hi: 64,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 186,
        m_hi: 22,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 243,
        m_hi: 86,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 244,
        m_hi: 87,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 190,
        m_hi: 34,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 189,
        m_hi: 42,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 245,
        m_hi: 73,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 195,
        m_hi: 31,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 251,
        m_hi: 94,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 252,
        m_hi: 95,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 198,
        m_hi: 42,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 197,
        m_hi: 50,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 253,
        m_hi: 81,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 203,
        m_hi: 39,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 104,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 107,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 206,
        m_hi: 50,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 205,
        m_hi: 58,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 102,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 211,
        m_hi: 47,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 113,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 123,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 214,
        m_hi: 58,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 213,
        m_hi: 66,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 126,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 219,
        m_hi: 55,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 126,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 253,
        m_hi: 172,
        m_trans: 28,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 223,
        m_hi: 67,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 222,
        m_hi: 75,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 153,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 174,
        m_hi: 2,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 242,
        m_hi: 66,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 254,
        m_hi: 182,
        m_trans: 28,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 231,
        m_hi: 75,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 230,
        m_hi: 83,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 178,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 182,
        m_hi: 10,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 246,
        m_hi: 79,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 190,
        m_trans: 28,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 239,
        m_hi: 83,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 238,
        m_hi: 91,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 202,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 190,
        m_hi: 18,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 251,
        m_hi: 89,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 251,
        m_hi: 88,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 247,
        m_hi: 91,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 246,
        m_hi: 99,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 227,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 198,
        m_hi: 26,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 102,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 108,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 102,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 108,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 207,
        m_hi: 35,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 117,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 132,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 117,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 132,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 215,
        m_hi: 43,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 254,
        m_hi: 167,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 156,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 254,
        m_hi: 167,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 156,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 223,
        m_hi: 51,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 175,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 181,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 175,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 181,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 231,
        m_hi: 59,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 105,
        m_hi: 0,
        m_trans: 47,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 105,
        m_hi: 0,
        m_trans: 44,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 33,
        m_hi: 0,
        m_trans: 15,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 33,
        m_hi: 0,
        m_trans: 12,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 106,
        m_hi: 33,
        m_trans: 48,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 0,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 115,
        m_hi: 0,
        m_trans: 47,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 114,
        m_hi: 2,
        m_trans: 44,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 41,
        m_hi: 0,
        m_trans: 15,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 41,
        m_hi: 0,
        m_trans: 12,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 114,
        m_hi: 5,
        m_trans: 32,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 0,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 123,
        m_hi: 2,
        m_trans: 47,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 124,
        m_hi: 3,
        m_trans: 44,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 49,
        m_hi: 0,
        m_trans: 15,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 49,
        m_hi: 0,
        m_trans: 12,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 122,
        m_hi: 13,
        m_trans: 32,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 0,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 190,
        m_hi: 0,
        m_trans: 111,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 190,
        m_hi: 0,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 57,
        m_hi: 0,
        m_trans: 15,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 57,
        m_hi: 0,
        m_trans: 12,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 130,
        m_hi: 21,
        m_trans: 32,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 0,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 205,
        m_hi: 0,
        m_trans: 111,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 205,
        m_hi: 0,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 66,
        m_hi: 0,
        m_trans: 15,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 66,
        m_hi: 0,
        m_trans: 12,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 139,
        m_hi: 30,
        m_trans: 32,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 0,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 217,
        m_hi: 4,
        m_trans: 111,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 215,
        m_hi: 7,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 74,
        m_hi: 4,
        m_trans: 15,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 74,
        m_hi: 8,
        m_trans: 12,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 218,
        m_hi: 3,
        m_trans: 96,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 8,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 229,
        m_hi: 8,
        m_trans: 111,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 223,
        m_hi: 15,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 80,
        m_hi: 0,
        m_trans: 11,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 82,
        m_hi: 16,
        m_trans: 12,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 226,
        m_hi: 11,
        m_trans: 96,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 16,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 237,
        m_hi: 13,
        m_trans: 111,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 231,
        m_hi: 23,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 89,
        m_hi: 0,
        m_trans: 11,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 90,
        m_hi: 24,
        m_trans: 12,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 234,
        m_hi: 19,
        m_trans: 96,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 24,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 251,
        m_hi: 17,
        m_trans: 111,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 240,
        m_hi: 32,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 99,
        m_hi: 0,
        m_trans: 11,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 99,
        m_hi: 1,
        m_trans: 8,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 243,
        m_hi: 28,
        m_trans: 96,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 33,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 169,
        m_hi: 0,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 248,
        m_hi: 40,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 108,
        m_hi: 2,
        m_trans: 11,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 107,
        m_hi: 9,
        m_trans: 8,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 251,
        m_hi: 36,
        m_trans: 96,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 41,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 181,
        m_hi: 0,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 254,
        m_hi: 49,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 166,
        m_hi: 0,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 115,
        m_hi: 17,
        m_trans: 8,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 188,
        m_hi: 79,
        m_trans: 32,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 49,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 190,
        m_hi: 0,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 59,
        m_trans: 108,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 181,
        m_hi: 0,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 123,
        m_hi: 25,
        m_trans: 8,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 196,
        m_hi: 87,
        m_trans: 32,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 57,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 203,
        m_hi: 0,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 203,
        m_hi: 0,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 197,
        m_hi: 0,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 196,
        m_hi: 2,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 205,
        m_hi: 96,
        m_trans: 32,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 66,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 213,
        m_hi: 1,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 212,
        m_hi: 3,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 211,
        m_hi: 2,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 204,
        m_hi: 10,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 213,
        m_hi: 104,
        m_trans: 32,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 74,
        m_hi: 1,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 221,
        m_hi: 9,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 220,
        m_hi: 11,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 219,
        m_hi: 10,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 212,
        m_hi: 18,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 221,
        m_hi: 0,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 82,
        m_hi: 9,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 229,
        m_hi: 17,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 228,
        m_hi: 19,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 227,
        m_hi: 18,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 220,
        m_hi: 26,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 229,
        m_hi: 5,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 90,
        m_hi: 17,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 238,
        m_hi: 26,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 237,
        m_hi: 28,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 236,
        m_hi: 27,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 229,
        m_hi: 35,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 238,
        m_hi: 14,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 99,
        m_hi: 26,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 246,
        m_hi: 34,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 245,
        m_hi: 36,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 244,
        m_hi: 35,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 237,
        m_hi: 43,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 246,
        m_hi: 22,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 107,
        m_hi: 34,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 254,
        m_hi: 42,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 253,
        m_hi: 44,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 252,
        m_hi: 43,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 245,
        m_hi: 51,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 254,
        m_hi: 30,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 115,
        m_hi: 6,
        m_trans: 2,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 52,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 58,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 52,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 253,
        m_hi: 59,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 53,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 123,
        m_hi: 14,
        m_trans: 2,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 64,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 74,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 196,
        m_hi: 0,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 198,
        m_hi: 100,
        m_trans: 8,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 80,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 132,
        m_hi: 23,
        m_trans: 2,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 74,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 89,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 206,
        m_hi: 1,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 206,
        m_hi: 4,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 105,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 140,
        m_hi: 31,
        m_trans: 2,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 86,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 253,
        m_hi: 147,
        m_trans: 28,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 215,
        m_hi: 7,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 214,
        m_hi: 12,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 129,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 219,
        m_hi: 4,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 95,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 155,
        m_trans: 28,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 223,
        m_hi: 15,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 222,
        m_hi: 20,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 153,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 227,
        m_hi: 12,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 242,
        m_hi: 17,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 166,
        m_trans: 28,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 232,
        m_hi: 24,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 231,
        m_hi: 29,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 181,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 236,
        m_hi: 21,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 247,
        m_hi: 26,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 175,
        m_trans: 28,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 240,
        m_hi: 32,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 239,
        m_hi: 37,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 205,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 244,
        m_hi: 29,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 251,
        m_hi: 38,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 251,
        m_hi: 37,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 248,
        m_hi: 40,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 247,
        m_hi: 45,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 230,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 252,
        m_hi: 37,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 50,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 53,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 50,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 53,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 189,
        m_hi: 80,
        m_trans: 2,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 65,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 80,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 65,
        m_trans: 6,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 80,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 198,
        m_hi: 89,
        m_trans: 2,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 252,
        m_hi: 131,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 105,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 252,
        m_hi: 131,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 105,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 206,
        m_hi: 97,
        m_trans: 2,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 254,
        m_hi: 139,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 129,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 253,
        m_hi: 141,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 129,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 214,
        m_hi: 105,
        m_trans: 2,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 150,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 153,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 150,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 153,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 222,
        m_hi: 0,
        m_trans: 1,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 181,
        m_hi: 0,
        m_trans: 47,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 178,
        m_hi: 0,
        m_trans: 44,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 47,
        m_hi: 0,
        m_trans: 15,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 47,
        m_hi: 0,
        m_trans: 12,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 183,
        m_hi: 47,
        m_trans: 48,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 0,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 190,
        m_hi: 0,
        m_trans: 47,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 190,
        m_hi: 0,
        m_trans: 44,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 55,
        m_hi: 0,
        m_trans: 15,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 55,
        m_hi: 0,
        m_trans: 12,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 191,
        m_hi: 55,
        m_trans: 48,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 0,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 199,
        m_hi: 0,
        m_trans: 47,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 199,
        m_hi: 0,
        m_trans: 44,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 63,
        m_hi: 0,
        m_trans: 15,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 63,
        m_hi: 0,
        m_trans: 12,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 199,
        m_hi: 63,
        m_trans: 48,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 0,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 207,
        m_hi: 0,
        m_trans: 47,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 207,
        m_hi: 1,
        m_trans: 44,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 71,
        m_hi: 0,
        m_trans: 15,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 71,
        m_hi: 0,
        m_trans: 12,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 207,
        m_hi: 4,
        m_trans: 32,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 0,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 216,
        m_hi: 1,
        m_trans: 47,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 218,
        m_hi: 3,
        m_trans: 44,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 80,
        m_hi: 0,
        m_trans: 15,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 80,
        m_hi: 0,
        m_trans: 12,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 216,
        m_hi: 13,
        m_trans: 32,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 0,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 225,
        m_hi: 1,
        m_trans: 47,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 227,
        m_hi: 6,
        m_trans: 44,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 88,
        m_hi: 0,
        m_trans: 15,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 88,
        m_hi: 0,
        m_trans: 12,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 224,
        m_hi: 21,
        m_trans: 32,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 0,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 233,
        m_hi: 2,
        m_trans: 47,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 235,
        m_hi: 11,
        m_trans: 44,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 96,
        m_hi: 1,
        m_trans: 15,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 96,
        m_hi: 2,
        m_trans: 12,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 232,
        m_hi: 29,
        m_trans: 32,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 2,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 241,
        m_hi: 4,
        m_trans: 47,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 243,
        m_hi: 19,
        m_trans: 44,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 104,
        m_hi: 5,
        m_trans: 15,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 104,
        m_hi: 10,
        m_trans: 12,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 240,
        m_hi: 37,
        m_trans: 32,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 10,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 251,
        m_hi: 5,
        m_trans: 47,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 252,
        m_hi: 28,
        m_trans: 44,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 113,
        m_hi: 9,
        m_trans: 15,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 113,
        m_hi: 19,
        m_trans: 12,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 249,
        m_hi: 46,
        m_trans: 32,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 19,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 9,
        m_trans: 47,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 37,
        m_trans: 44,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 117,
        m_hi: 0,
        m_trans: 11,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 121,
        m_hi: 27,
        m_trans: 12,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 55,
        m_trans: 32,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 27,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 9,
        m_trans: 47,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 46,
        m_trans: 44,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 126,
        m_hi: 0,
        m_trans: 11,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 129,
        m_hi: 35,
        m_trans: 12,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 67,
        m_trans: 32,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 35,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 248,
        m_hi: 0,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 55,
        m_trans: 44,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 137,
        m_hi: 0,
        m_trans: 11,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 137,
        m_hi: 43,
        m_trans: 12,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 79,
        m_trans: 32,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 43,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 250,
        m_hi: 0,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 64,
        m_trans: 44,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 147,
        m_hi: 2,
        m_trans: 11,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 146,
        m_hi: 6,
        m_trans: 8,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 93,
        m_trans: 32,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 52,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 251,
        m_hi: 0,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 73,
        m_trans: 44,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 155,
        m_hi: 4,
        m_trans: 11,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 154,
        m_hi: 14,
        m_trans: 8,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 104,
        m_trans: 32,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 60,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 253,
        m_hi: 0,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 248,
        m_hi: 0,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 233,
        m_hi: 0,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 162,
        m_hi: 22,
        m_trans: 8,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 116,
        m_trans: 32,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 68,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 254,
        m_hi: 0,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 254,
        m_hi: 0,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 248,
        m_hi: 0,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 170,
        m_hi: 30,
        m_trans: 8,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 128,
        m_trans: 32,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 76,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 1,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 7,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 1,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 179,
        m_hi: 39,
        m_trans: 8,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 22,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 85,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 2,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 22,
        m_trans: 24,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 7,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 187,
        m_hi: 47,
        m_trans: 8,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 47,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 93,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 4,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 251,
        m_hi: 100,
        m_trans: 28,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 182,
        m_hi: 0,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 195,
        m_hi: 55,
        m_trans: 8,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 71,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 101,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 4,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 253,
        m_hi: 108,
        m_trans: 28,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 191,
        m_hi: 0,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 203,
        m_hi: 63,
        m_trans: 8,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 95,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 109,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 7,
        m_trans: 27,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 118,
        m_trans: 28,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 200,
        m_hi: 0,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 212,
        m_hi: 72,
        m_trans: 8,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 123,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 118,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 246,
        m_hi: 0,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 129,
        m_trans: 28,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 209,
        m_hi: 0,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 220,
        m_hi: 80,
        m_trans: 8,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 147,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 126,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 246,
        m_hi: 0,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 138,
        m_trans: 28,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 218,
        m_hi: 0,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 228,
        m_hi: 88,
        m_trans: 8,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 172,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 134,
        m_hi: 0,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 249,
        m_hi: 3,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 245,
        m_hi: 91,
        m_trans: 8,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 228,
        m_hi: 3,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 236,
        m_hi: 96,
        m_trans: 8,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 196,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 142,
        m_hi: 6,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 251,
        m_hi: 14,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 250,
        m_hi: 102,
        m_trans: 8,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 237,
        m_hi: 12,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 245,
        m_hi: 105,
        m_trans: 8,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 223,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 151,
        m_hi: 15,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 253,
        m_hi: 22,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 254,
        m_hi: 112,
        m_trans: 8,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 245,
        m_hi: 20,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 253,
        m_hi: 113,
        m_trans: 8,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 248,
        m_trans: 16,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 159,
        m_hi: 23,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 253,
        m_hi: 31,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 124,
        m_trans: 8,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 249,
        m_hi: 28,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 124,
        m_trans: 8,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 167,
        m_hi: 31,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 254,
        m_hi: 39,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 10,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 252,
        m_hi: 37,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 10,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 175,
        m_hi: 39,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 48,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 38,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 254,
        m_hi: 48,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 38,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 184,
        m_hi: 48,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 56,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 62,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 56,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 62,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 192,
        m_hi: 56,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 65,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 86,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 65,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 86,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 200,
        m_hi: 64,
        m_trans: 3,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 74,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 111,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 77,
        m_trans: 7,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 111,
        m_trans: 4,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 255,
        m_hi: 0,
        m_trans: 0,
    },
    Etc1GToBc7M5aConversion {
        m_lo: 208,
        m_hi: 5,
        m_trans: 2,
    },
];
