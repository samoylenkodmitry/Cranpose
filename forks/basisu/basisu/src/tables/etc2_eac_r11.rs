//! ETC1 grayscale to ETC2 EAC R11 conversion table, indexed
//! `[base + inten * 32][selector_range]`. Each entry carries the R11 base value,
//! the packed `table * 16 + multiplier`, and the selector translation. It reuses
//! the [`Etc1GToEac`] entry type from the EAC-A8 table.

use super::etc2_eac_a8::Etc1GToEac;

/// The conversion table, indexed `[base + inten * 32][selector_range]`.
pub static S_ETC1_G_TO_ETC2_R11: [[Etc1GToEac; 4]; 256] = [
    [
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 1,
            m_trans: 3328,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 1,
            m_trans: 3328,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 16,
            m_trans: 457,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 16,
            m_trans: 456,
        },
    ],
    [
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 226,
            m_trans: 3936,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 226,
            m_trans: 3936,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 17,
            m_trans: 424,
        },
        Etc1GToEac {
            m_base: 8,
            m_table_mul: 0,
            m_trans: 472,
        },
    ],
    [
        Etc1GToEac {
            m_base: 6,
            m_table_mul: 178,
            m_trans: 4012,
        },
        Etc1GToEac {
            m_base: 6,
            m_table_mul: 178,
            m_trans: 4008,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 146,
            m_trans: 501,
        },
        Etc1GToEac {
            m_base: 16,
            m_table_mul: 0,
            m_trans: 472,
        },
    ],
    [
        Etc1GToEac {
            m_base: 14,
            m_table_mul: 178,
            m_trans: 4012,
        },
        Etc1GToEac {
            m_base: 14,
            m_table_mul: 178,
            m_trans: 4008,
        },
        Etc1GToEac {
            m_base: 8,
            m_table_mul: 146,
            m_trans: 501,
        },
        Etc1GToEac {
            m_base: 24,
            m_table_mul: 0,
            m_trans: 472,
        },
    ],
    [
        Etc1GToEac {
            m_base: 23,
            m_table_mul: 178,
            m_trans: 4012,
        },
        Etc1GToEac {
            m_base: 23,
            m_table_mul: 178,
            m_trans: 4008,
        },
        Etc1GToEac {
            m_base: 17,
            m_table_mul: 146,
            m_trans: 501,
        },
        Etc1GToEac {
            m_base: 33,
            m_table_mul: 0,
            m_trans: 472,
        },
    ],
    [
        Etc1GToEac {
            m_base: 31,
            m_table_mul: 178,
            m_trans: 4012,
        },
        Etc1GToEac {
            m_base: 31,
            m_table_mul: 178,
            m_trans: 4008,
        },
        Etc1GToEac {
            m_base: 25,
            m_table_mul: 146,
            m_trans: 501,
        },
        Etc1GToEac {
            m_base: 41,
            m_table_mul: 0,
            m_trans: 472,
        },
    ],
    [
        Etc1GToEac {
            m_base: 39,
            m_table_mul: 178,
            m_trans: 4012,
        },
        Etc1GToEac {
            m_base: 39,
            m_table_mul: 178,
            m_trans: 4008,
        },
        Etc1GToEac {
            m_base: 33,
            m_table_mul: 146,
            m_trans: 501,
        },
        Etc1GToEac {
            m_base: 49,
            m_table_mul: 0,
            m_trans: 472,
        },
    ],
    [
        Etc1GToEac {
            m_base: 47,
            m_table_mul: 178,
            m_trans: 4012,
        },
        Etc1GToEac {
            m_base: 47,
            m_table_mul: 178,
            m_trans: 4008,
        },
        Etc1GToEac {
            m_base: 41,
            m_table_mul: 146,
            m_trans: 501,
        },
        Etc1GToEac {
            m_base: 27,
            m_table_mul: 228,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 56,
            m_table_mul: 178,
            m_trans: 4012,
        },
        Etc1GToEac {
            m_base: 56,
            m_table_mul: 178,
            m_trans: 4008,
        },
        Etc1GToEac {
            m_base: 50,
            m_table_mul: 146,
            m_trans: 501,
        },
        Etc1GToEac {
            m_base: 36,
            m_table_mul: 228,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 64,
            m_table_mul: 178,
            m_trans: 4012,
        },
        Etc1GToEac {
            m_base: 64,
            m_table_mul: 178,
            m_trans: 4008,
        },
        Etc1GToEac {
            m_base: 58,
            m_table_mul: 146,
            m_trans: 501,
        },
        Etc1GToEac {
            m_base: 44,
            m_table_mul: 228,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 72,
            m_table_mul: 178,
            m_trans: 4012,
        },
        Etc1GToEac {
            m_base: 72,
            m_table_mul: 178,
            m_trans: 4008,
        },
        Etc1GToEac {
            m_base: 66,
            m_table_mul: 146,
            m_trans: 501,
        },
        Etc1GToEac {
            m_base: 52,
            m_table_mul: 228,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 80,
            m_table_mul: 178,
            m_trans: 4012,
        },
        Etc1GToEac {
            m_base: 80,
            m_table_mul: 178,
            m_trans: 4008,
        },
        Etc1GToEac {
            m_base: 74,
            m_table_mul: 146,
            m_trans: 501,
        },
        Etc1GToEac {
            m_base: 60,
            m_table_mul: 228,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 89,
            m_table_mul: 178,
            m_trans: 4012,
        },
        Etc1GToEac {
            m_base: 89,
            m_table_mul: 178,
            m_trans: 4008,
        },
        Etc1GToEac {
            m_base: 83,
            m_table_mul: 146,
            m_trans: 501,
        },
        Etc1GToEac {
            m_base: 69,
            m_table_mul: 228,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 97,
            m_table_mul: 178,
            m_trans: 4012,
        },
        Etc1GToEac {
            m_base: 97,
            m_table_mul: 178,
            m_trans: 4008,
        },
        Etc1GToEac {
            m_base: 91,
            m_table_mul: 146,
            m_trans: 501,
        },
        Etc1GToEac {
            m_base: 77,
            m_table_mul: 228,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 105,
            m_table_mul: 178,
            m_trans: 4012,
        },
        Etc1GToEac {
            m_base: 105,
            m_table_mul: 178,
            m_trans: 4008,
        },
        Etc1GToEac {
            m_base: 99,
            m_table_mul: 146,
            m_trans: 501,
        },
        Etc1GToEac {
            m_base: 85,
            m_table_mul: 228,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 113,
            m_table_mul: 178,
            m_trans: 4012,
        },
        Etc1GToEac {
            m_base: 113,
            m_table_mul: 178,
            m_trans: 4008,
        },
        Etc1GToEac {
            m_base: 107,
            m_table_mul: 146,
            m_trans: 501,
        },
        Etc1GToEac {
            m_base: 93,
            m_table_mul: 228,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 122,
            m_table_mul: 178,
            m_trans: 4012,
        },
        Etc1GToEac {
            m_base: 122,
            m_table_mul: 178,
            m_trans: 4008,
        },
        Etc1GToEac {
            m_base: 116,
            m_table_mul: 146,
            m_trans: 501,
        },
        Etc1GToEac {
            m_base: 102,
            m_table_mul: 228,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 130,
            m_table_mul: 178,
            m_trans: 4012,
        },
        Etc1GToEac {
            m_base: 130,
            m_table_mul: 178,
            m_trans: 4008,
        },
        Etc1GToEac {
            m_base: 124,
            m_table_mul: 146,
            m_trans: 501,
        },
        Etc1GToEac {
            m_base: 110,
            m_table_mul: 228,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 138,
            m_table_mul: 178,
            m_trans: 4012,
        },
        Etc1GToEac {
            m_base: 138,
            m_table_mul: 178,
            m_trans: 4008,
        },
        Etc1GToEac {
            m_base: 132,
            m_table_mul: 146,
            m_trans: 501,
        },
        Etc1GToEac {
            m_base: 118,
            m_table_mul: 228,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 146,
            m_table_mul: 178,
            m_trans: 4012,
        },
        Etc1GToEac {
            m_base: 146,
            m_table_mul: 178,
            m_trans: 4008,
        },
        Etc1GToEac {
            m_base: 140,
            m_table_mul: 146,
            m_trans: 501,
        },
        Etc1GToEac {
            m_base: 126,
            m_table_mul: 228,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 155,
            m_table_mul: 178,
            m_trans: 4012,
        },
        Etc1GToEac {
            m_base: 155,
            m_table_mul: 178,
            m_trans: 4008,
        },
        Etc1GToEac {
            m_base: 149,
            m_table_mul: 146,
            m_trans: 501,
        },
        Etc1GToEac {
            m_base: 135,
            m_table_mul: 228,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 163,
            m_table_mul: 178,
            m_trans: 4012,
        },
        Etc1GToEac {
            m_base: 163,
            m_table_mul: 178,
            m_trans: 4008,
        },
        Etc1GToEac {
            m_base: 157,
            m_table_mul: 146,
            m_trans: 501,
        },
        Etc1GToEac {
            m_base: 143,
            m_table_mul: 228,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 171,
            m_table_mul: 178,
            m_trans: 4012,
        },
        Etc1GToEac {
            m_base: 171,
            m_table_mul: 178,
            m_trans: 4008,
        },
        Etc1GToEac {
            m_base: 165,
            m_table_mul: 146,
            m_trans: 501,
        },
        Etc1GToEac {
            m_base: 151,
            m_table_mul: 228,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 179,
            m_table_mul: 178,
            m_trans: 4012,
        },
        Etc1GToEac {
            m_base: 179,
            m_table_mul: 178,
            m_trans: 4008,
        },
        Etc1GToEac {
            m_base: 173,
            m_table_mul: 146,
            m_trans: 501,
        },
        Etc1GToEac {
            m_base: 159,
            m_table_mul: 228,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 188,
            m_table_mul: 178,
            m_trans: 4012,
        },
        Etc1GToEac {
            m_base: 188,
            m_table_mul: 178,
            m_trans: 4008,
        },
        Etc1GToEac {
            m_base: 182,
            m_table_mul: 146,
            m_trans: 501,
        },
        Etc1GToEac {
            m_base: 168,
            m_table_mul: 228,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 196,
            m_table_mul: 178,
            m_trans: 4012,
        },
        Etc1GToEac {
            m_base: 196,
            m_table_mul: 178,
            m_trans: 4008,
        },
        Etc1GToEac {
            m_base: 190,
            m_table_mul: 146,
            m_trans: 501,
        },
        Etc1GToEac {
            m_base: 176,
            m_table_mul: 228,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 204,
            m_table_mul: 178,
            m_trans: 4012,
        },
        Etc1GToEac {
            m_base: 204,
            m_table_mul: 178,
            m_trans: 4008,
        },
        Etc1GToEac {
            m_base: 198,
            m_table_mul: 146,
            m_trans: 501,
        },
        Etc1GToEac {
            m_base: 184,
            m_table_mul: 228,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 212,
            m_table_mul: 178,
            m_trans: 4012,
        },
        Etc1GToEac {
            m_base: 212,
            m_table_mul: 178,
            m_trans: 4008,
        },
        Etc1GToEac {
            m_base: 206,
            m_table_mul: 146,
            m_trans: 501,
        },
        Etc1GToEac {
            m_base: 192,
            m_table_mul: 228,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 221,
            m_table_mul: 178,
            m_trans: 4012,
        },
        Etc1GToEac {
            m_base: 221,
            m_table_mul: 178,
            m_trans: 4008,
        },
        Etc1GToEac {
            m_base: 215,
            m_table_mul: 146,
            m_trans: 501,
        },
        Etc1GToEac {
            m_base: 201,
            m_table_mul: 228,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 229,
            m_table_mul: 178,
            m_trans: 4012,
        },
        Etc1GToEac {
            m_base: 229,
            m_table_mul: 178,
            m_trans: 4008,
        },
        Etc1GToEac {
            m_base: 223,
            m_table_mul: 146,
            m_trans: 501,
        },
        Etc1GToEac {
            m_base: 209,
            m_table_mul: 228,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 235,
            m_table_mul: 66,
            m_trans: 4012,
        },
        Etc1GToEac {
            m_base: 221,
            m_table_mul: 100,
            m_trans: 4008,
        },
        Etc1GToEac {
            m_base: 231,
            m_table_mul: 146,
            m_trans: 501,
        },
        Etc1GToEac {
            m_base: 217,
            m_table_mul: 228,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 211,
            m_table_mul: 102,
            m_trans: 4085,
        },
        Etc1GToEac {
            m_base: 254,
            m_table_mul: 32,
            m_trans: 4040,
        },
        Etc1GToEac {
            m_base: 211,
            m_table_mul: 102,
            m_trans: 501,
        },
        Etc1GToEac {
            m_base: 254,
            m_table_mul: 32,
            m_trans: 456,
        },
    ],
    [
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 2,
            m_trans: 3328,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 2,
            m_trans: 3328,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 1,
            m_trans: 320,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 1,
            m_trans: 320,
        },
    ],
    [
        Etc1GToEac {
            m_base: 7,
            m_table_mul: 162,
            m_trans: 3905,
        },
        Etc1GToEac {
            m_base: 7,
            m_table_mul: 162,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 17,
            m_trans: 480,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 17,
            m_trans: 480,
        },
    ],
    [
        Etc1GToEac {
            m_base: 15,
            m_table_mul: 162,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 15,
            m_table_mul: 162,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 1,
            m_table_mul: 117,
            m_trans: 352,
        },
        Etc1GToEac {
            m_base: 1,
            m_table_mul: 117,
            m_trans: 352,
        },
    ],
    [
        Etc1GToEac {
            m_base: 23,
            m_table_mul: 162,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 23,
            m_table_mul: 162,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 5,
            m_table_mul: 34,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 4,
            m_table_mul: 53,
            m_trans: 424,
        },
    ],
    [
        Etc1GToEac {
            m_base: 32,
            m_table_mul: 162,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 32,
            m_table_mul: 162,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 14,
            m_table_mul: 34,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 3,
            m_table_mul: 69,
            m_trans: 424,
        },
    ],
    [
        Etc1GToEac {
            m_base: 40,
            m_table_mul: 162,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 40,
            m_table_mul: 162,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 22,
            m_table_mul: 34,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 1,
            m_table_mul: 133,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 48,
            m_table_mul: 162,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 48,
            m_table_mul: 162,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 30,
            m_table_mul: 34,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 4,
            m_table_mul: 85,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 56,
            m_table_mul: 162,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 56,
            m_table_mul: 162,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 38,
            m_table_mul: 34,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 12,
            m_table_mul: 85,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 65,
            m_table_mul: 162,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 65,
            m_table_mul: 162,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 47,
            m_table_mul: 34,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 1,
            m_table_mul: 106,
            m_trans: 424,
        },
    ],
    [
        Etc1GToEac {
            m_base: 73,
            m_table_mul: 162,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 73,
            m_table_mul: 162,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 55,
            m_table_mul: 34,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 9,
            m_table_mul: 106,
            m_trans: 424,
        },
    ],
    [
        Etc1GToEac {
            m_base: 81,
            m_table_mul: 162,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 81,
            m_table_mul: 162,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 63,
            m_table_mul: 34,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 7,
            m_table_mul: 234,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 89,
            m_table_mul: 162,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 89,
            m_table_mul: 162,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 71,
            m_table_mul: 34,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 15,
            m_table_mul: 234,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 98,
            m_table_mul: 162,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 98,
            m_table_mul: 162,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 80,
            m_table_mul: 34,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 24,
            m_table_mul: 234,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 106,
            m_table_mul: 162,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 106,
            m_table_mul: 162,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 88,
            m_table_mul: 34,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 32,
            m_table_mul: 234,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 114,
            m_table_mul: 162,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 114,
            m_table_mul: 162,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 96,
            m_table_mul: 34,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 40,
            m_table_mul: 234,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 122,
            m_table_mul: 162,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 122,
            m_table_mul: 162,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 104,
            m_table_mul: 34,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 48,
            m_table_mul: 234,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 131,
            m_table_mul: 162,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 131,
            m_table_mul: 162,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 113,
            m_table_mul: 34,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 57,
            m_table_mul: 234,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 139,
            m_table_mul: 162,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 139,
            m_table_mul: 162,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 121,
            m_table_mul: 34,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 65,
            m_table_mul: 234,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 147,
            m_table_mul: 162,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 147,
            m_table_mul: 162,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 129,
            m_table_mul: 34,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 73,
            m_table_mul: 234,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 155,
            m_table_mul: 162,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 155,
            m_table_mul: 162,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 137,
            m_table_mul: 34,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 81,
            m_table_mul: 234,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 164,
            m_table_mul: 162,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 164,
            m_table_mul: 162,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 146,
            m_table_mul: 34,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 90,
            m_table_mul: 234,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 172,
            m_table_mul: 162,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 172,
            m_table_mul: 162,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 154,
            m_table_mul: 34,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 98,
            m_table_mul: 234,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 180,
            m_table_mul: 162,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 180,
            m_table_mul: 162,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 162,
            m_table_mul: 34,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 106,
            m_table_mul: 234,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 188,
            m_table_mul: 162,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 188,
            m_table_mul: 162,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 170,
            m_table_mul: 34,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 114,
            m_table_mul: 234,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 197,
            m_table_mul: 162,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 197,
            m_table_mul: 162,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 179,
            m_table_mul: 34,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 123,
            m_table_mul: 234,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 205,
            m_table_mul: 162,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 205,
            m_table_mul: 162,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 187,
            m_table_mul: 34,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 131,
            m_table_mul: 234,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 213,
            m_table_mul: 162,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 213,
            m_table_mul: 162,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 195,
            m_table_mul: 34,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 139,
            m_table_mul: 234,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 221,
            m_table_mul: 162,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 221,
            m_table_mul: 162,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 203,
            m_table_mul: 34,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 147,
            m_table_mul: 234,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 230,
            m_table_mul: 162,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 230,
            m_table_mul: 162,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 212,
            m_table_mul: 34,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 156,
            m_table_mul: 234,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 238,
            m_table_mul: 162,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 174,
            m_table_mul: 106,
            m_trans: 4008,
        },
        Etc1GToEac {
            m_base: 220,
            m_table_mul: 34,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 164,
            m_table_mul: 234,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 240,
            m_table_mul: 178,
            m_trans: 4001,
        },
        Etc1GToEac {
            m_base: 182,
            m_table_mul: 106,
            m_trans: 4008,
        },
        Etc1GToEac {
            m_base: 228,
            m_table_mul: 34,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 172,
            m_table_mul: 234,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 166,
            m_table_mul: 108,
            m_trans: 4085,
        },
        Etc1GToEac {
            m_base: 115,
            m_table_mul: 31,
            m_trans: 4080,
        },
        Etc1GToEac {
            m_base: 166,
            m_table_mul: 108,
            m_trans: 501,
        },
        Etc1GToEac {
            m_base: 115,
            m_table_mul: 31,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 1,
            m_table_mul: 68,
            m_trans: 3328,
        },
        Etc1GToEac {
            m_base: 1,
            m_table_mul: 68,
            m_trans: 3328,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 1,
            m_trans: 384,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 1,
            m_trans: 384,
        },
    ],
    [
        Etc1GToEac {
            m_base: 1,
            m_table_mul: 51,
            m_trans: 3968,
        },
        Etc1GToEac {
            m_base: 1,
            m_table_mul: 51,
            m_trans: 3968,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 2,
            m_trans: 384,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 2,
            m_trans: 384,
        },
    ],
    [
        Etc1GToEac {
            m_base: 21,
            m_table_mul: 18,
            m_trans: 3851,
        },
        Etc1GToEac {
            m_base: 21,
            m_table_mul: 18,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 1,
            m_table_mul: 50,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 1,
            m_table_mul: 50,
            m_trans: 488,
        },
    ],
    [
        Etc1GToEac {
            m_base: 26,
            m_table_mul: 195,
            m_trans: 3851,
        },
        Etc1GToEac {
            m_base: 29,
            m_table_mul: 18,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 67,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 67,
            m_trans: 488,
        },
    ],
    [
        Etc1GToEac {
            m_base: 35,
            m_table_mul: 195,
            m_trans: 3851,
        },
        Etc1GToEac {
            m_base: 38,
            m_table_mul: 18,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 12,
            m_table_mul: 115,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 3,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 43,
            m_table_mul: 195,
            m_trans: 3851,
        },
        Etc1GToEac {
            m_base: 46,
            m_table_mul: 18,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 20,
            m_table_mul: 115,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 2,
            m_table_mul: 6,
            m_trans: 424,
        },
    ],
    [
        Etc1GToEac {
            m_base: 51,
            m_table_mul: 195,
            m_trans: 3851,
        },
        Etc1GToEac {
            m_base: 54,
            m_table_mul: 18,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 36,
            m_table_mul: 66,
            m_trans: 482,
        },
        Etc1GToEac {
            m_base: 4,
            m_table_mul: 22,
            m_trans: 424,
        },
    ],
    [
        Etc1GToEac {
            m_base: 59,
            m_table_mul: 195,
            m_trans: 3851,
        },
        Etc1GToEac {
            m_base: 62,
            m_table_mul: 18,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 44,
            m_table_mul: 66,
            m_trans: 482,
        },
        Etc1GToEac {
            m_base: 3,
            m_table_mul: 73,
            m_trans: 424,
        },
    ],
    [
        Etc1GToEac {
            m_base: 68,
            m_table_mul: 195,
            m_trans: 3851,
        },
        Etc1GToEac {
            m_base: 71,
            m_table_mul: 18,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 53,
            m_table_mul: 66,
            m_trans: 482,
        },
        Etc1GToEac {
            m_base: 3,
            m_table_mul: 22,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 76,
            m_table_mul: 195,
            m_trans: 3851,
        },
        Etc1GToEac {
            m_base: 79,
            m_table_mul: 18,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 61,
            m_table_mul: 66,
            m_trans: 482,
        },
        Etc1GToEac {
            m_base: 2,
            m_table_mul: 137,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 84,
            m_table_mul: 195,
            m_trans: 3851,
        },
        Etc1GToEac {
            m_base: 87,
            m_table_mul: 18,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 69,
            m_table_mul: 66,
            m_trans: 482,
        },
        Etc1GToEac {
            m_base: 1,
            m_table_mul: 89,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 92,
            m_table_mul: 195,
            m_trans: 3851,
        },
        Etc1GToEac {
            m_base: 95,
            m_table_mul: 18,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 77,
            m_table_mul: 66,
            m_trans: 482,
        },
        Etc1GToEac {
            m_base: 9,
            m_table_mul: 89,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 101,
            m_table_mul: 195,
            m_trans: 3851,
        },
        Etc1GToEac {
            m_base: 104,
            m_table_mul: 18,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 86,
            m_table_mul: 66,
            m_trans: 482,
        },
        Etc1GToEac {
            m_base: 18,
            m_table_mul: 89,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 109,
            m_table_mul: 195,
            m_trans: 3851,
        },
        Etc1GToEac {
            m_base: 112,
            m_table_mul: 18,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 94,
            m_table_mul: 66,
            m_trans: 482,
        },
        Etc1GToEac {
            m_base: 26,
            m_table_mul: 89,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 117,
            m_table_mul: 195,
            m_trans: 3851,
        },
        Etc1GToEac {
            m_base: 120,
            m_table_mul: 18,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 102,
            m_table_mul: 66,
            m_trans: 482,
        },
        Etc1GToEac {
            m_base: 34,
            m_table_mul: 89,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 125,
            m_table_mul: 195,
            m_trans: 3851,
        },
        Etc1GToEac {
            m_base: 128,
            m_table_mul: 18,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 110,
            m_table_mul: 66,
            m_trans: 482,
        },
        Etc1GToEac {
            m_base: 42,
            m_table_mul: 89,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 134,
            m_table_mul: 195,
            m_trans: 3851,
        },
        Etc1GToEac {
            m_base: 137,
            m_table_mul: 18,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 119,
            m_table_mul: 66,
            m_trans: 482,
        },
        Etc1GToEac {
            m_base: 51,
            m_table_mul: 89,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 141,
            m_table_mul: 195,
            m_trans: 3907,
        },
        Etc1GToEac {
            m_base: 145,
            m_table_mul: 18,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 127,
            m_table_mul: 66,
            m_trans: 482,
        },
        Etc1GToEac {
            m_base: 59,
            m_table_mul: 89,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 149,
            m_table_mul: 195,
            m_trans: 3907,
        },
        Etc1GToEac {
            m_base: 153,
            m_table_mul: 18,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 135,
            m_table_mul: 66,
            m_trans: 482,
        },
        Etc1GToEac {
            m_base: 67,
            m_table_mul: 89,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 157,
            m_table_mul: 195,
            m_trans: 3907,
        },
        Etc1GToEac {
            m_base: 161,
            m_table_mul: 18,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 143,
            m_table_mul: 66,
            m_trans: 482,
        },
        Etc1GToEac {
            m_base: 75,
            m_table_mul: 89,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 166,
            m_table_mul: 195,
            m_trans: 3907,
        },
        Etc1GToEac {
            m_base: 170,
            m_table_mul: 18,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 152,
            m_table_mul: 66,
            m_trans: 482,
        },
        Etc1GToEac {
            m_base: 84,
            m_table_mul: 89,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 174,
            m_table_mul: 195,
            m_trans: 3907,
        },
        Etc1GToEac {
            m_base: 178,
            m_table_mul: 18,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 160,
            m_table_mul: 66,
            m_trans: 482,
        },
        Etc1GToEac {
            m_base: 92,
            m_table_mul: 89,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 182,
            m_table_mul: 195,
            m_trans: 3907,
        },
        Etc1GToEac {
            m_base: 186,
            m_table_mul: 18,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 168,
            m_table_mul: 66,
            m_trans: 482,
        },
        Etc1GToEac {
            m_base: 100,
            m_table_mul: 89,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 190,
            m_table_mul: 195,
            m_trans: 3907,
        },
        Etc1GToEac {
            m_base: 194,
            m_table_mul: 18,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 176,
            m_table_mul: 66,
            m_trans: 482,
        },
        Etc1GToEac {
            m_base: 108,
            m_table_mul: 89,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 199,
            m_table_mul: 195,
            m_trans: 3907,
        },
        Etc1GToEac {
            m_base: 203,
            m_table_mul: 18,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 185,
            m_table_mul: 66,
            m_trans: 482,
        },
        Etc1GToEac {
            m_base: 117,
            m_table_mul: 89,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 207,
            m_table_mul: 195,
            m_trans: 3907,
        },
        Etc1GToEac {
            m_base: 211,
            m_table_mul: 18,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 193,
            m_table_mul: 66,
            m_trans: 482,
        },
        Etc1GToEac {
            m_base: 125,
            m_table_mul: 89,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 215,
            m_table_mul: 195,
            m_trans: 3907,
        },
        Etc1GToEac {
            m_base: 219,
            m_table_mul: 18,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 201,
            m_table_mul: 66,
            m_trans: 482,
        },
        Etc1GToEac {
            m_base: 133,
            m_table_mul: 89,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 223,
            m_table_mul: 195,
            m_trans: 3907,
        },
        Etc1GToEac {
            m_base: 227,
            m_table_mul: 18,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 209,
            m_table_mul: 66,
            m_trans: 482,
        },
        Etc1GToEac {
            m_base: 141,
            m_table_mul: 89,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 232,
            m_table_mul: 195,
            m_trans: 3907,
        },
        Etc1GToEac {
            m_base: 168,
            m_table_mul: 89,
            m_trans: 4008,
        },
        Etc1GToEac {
            m_base: 218,
            m_table_mul: 66,
            m_trans: 482,
        },
        Etc1GToEac {
            m_base: 150,
            m_table_mul: 89,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 236,
            m_table_mul: 18,
            m_trans: 3907,
        },
        Etc1GToEac {
            m_base: 176,
            m_table_mul: 89,
            m_trans: 4008,
        },
        Etc1GToEac {
            m_base: 226,
            m_table_mul: 66,
            m_trans: 482,
        },
        Etc1GToEac {
            m_base: 158,
            m_table_mul: 89,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 158,
            m_table_mul: 90,
            m_trans: 4085,
        },
        Etc1GToEac {
            m_base: 103,
            m_table_mul: 31,
            m_trans: 4080,
        },
        Etc1GToEac {
            m_base: 158,
            m_table_mul: 90,
            m_trans: 501,
        },
        Etc1GToEac {
            m_base: 103,
            m_table_mul: 31,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 166,
            m_table_mul: 90,
            m_trans: 4085,
        },
        Etc1GToEac {
            m_base: 111,
            m_table_mul: 31,
            m_trans: 4080,
        },
        Etc1GToEac {
            m_base: 166,
            m_table_mul: 90,
            m_trans: 501,
        },
        Etc1GToEac {
            m_base: 111,
            m_table_mul: 31,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 70,
            m_trans: 3328,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 70,
            m_trans: 3328,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 17,
            m_trans: 448,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 17,
            m_trans: 448,
        },
    ],
    [
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 117,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 117,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 35,
            m_trans: 384,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 35,
            m_trans: 384,
        },
    ],
    [
        Etc1GToEac {
            m_base: 13,
            m_table_mul: 165,
            m_trans: 3905,
        },
        Etc1GToEac {
            m_base: 13,
            m_table_mul: 165,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 2,
            m_table_mul: 211,
            m_trans: 480,
        },
        Etc1GToEac {
            m_base: 2,
            m_table_mul: 211,
            m_trans: 480,
        },
    ],
    [
        Etc1GToEac {
            m_base: 21,
            m_table_mul: 165,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 21,
            m_table_mul: 165,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 1,
            m_table_mul: 51,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 1,
            m_table_mul: 51,
            m_trans: 488,
        },
    ],
    [
        Etc1GToEac {
            m_base: 30,
            m_table_mul: 165,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 30,
            m_table_mul: 165,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 7,
            m_table_mul: 61,
            m_trans: 352,
        },
        Etc1GToEac {
            m_base: 7,
            m_table_mul: 61,
            m_trans: 352,
        },
    ],
    [
        Etc1GToEac {
            m_base: 38,
            m_table_mul: 165,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 38,
            m_table_mul: 165,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 2,
            m_table_mul: 125,
            m_trans: 352,
        },
        Etc1GToEac {
            m_base: 2,
            m_table_mul: 125,
            m_trans: 352,
        },
    ],
    [
        Etc1GToEac {
            m_base: 46,
            m_table_mul: 165,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 46,
            m_table_mul: 165,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 1,
            m_table_mul: 37,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 10,
            m_table_mul: 125,
            m_trans: 352,
        },
    ],
    [
        Etc1GToEac {
            m_base: 54,
            m_table_mul: 165,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 54,
            m_table_mul: 165,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 9,
            m_table_mul: 37,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 5,
            m_table_mul: 61,
            m_trans: 424,
        },
    ],
    [
        Etc1GToEac {
            m_base: 63,
            m_table_mul: 165,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 63,
            m_table_mul: 165,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 18,
            m_table_mul: 37,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 1,
            m_table_mul: 189,
            m_trans: 424,
        },
    ],
    [
        Etc1GToEac {
            m_base: 71,
            m_table_mul: 165,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 71,
            m_table_mul: 165,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 26,
            m_table_mul: 37,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 9,
            m_table_mul: 189,
            m_trans: 424,
        },
    ],
    [
        Etc1GToEac {
            m_base: 79,
            m_table_mul: 165,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 79,
            m_table_mul: 165,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 34,
            m_table_mul: 37,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 4,
            m_table_mul: 77,
            m_trans: 424,
        },
    ],
    [
        Etc1GToEac {
            m_base: 87,
            m_table_mul: 165,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 87,
            m_table_mul: 165,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 42,
            m_table_mul: 37,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 12,
            m_table_mul: 77,
            m_trans: 424,
        },
    ],
    [
        Etc1GToEac {
            m_base: 96,
            m_table_mul: 165,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 96,
            m_table_mul: 165,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 51,
            m_table_mul: 37,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 8,
            m_table_mul: 93,
            m_trans: 424,
        },
    ],
    [
        Etc1GToEac {
            m_base: 104,
            m_table_mul: 165,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 104,
            m_table_mul: 165,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 59,
            m_table_mul: 37,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 3,
            m_table_mul: 141,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 112,
            m_table_mul: 165,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 112,
            m_table_mul: 165,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 68,
            m_table_mul: 37,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 11,
            m_table_mul: 141,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 120,
            m_table_mul: 165,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 120,
            m_table_mul: 165,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 76,
            m_table_mul: 37,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 6,
            m_table_mul: 93,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 129,
            m_table_mul: 165,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 129,
            m_table_mul: 165,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 85,
            m_table_mul: 37,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 15,
            m_table_mul: 93,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 70,
            m_table_mul: 254,
            m_trans: 4012,
        },
        Etc1GToEac {
            m_base: 137,
            m_table_mul: 165,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 93,
            m_table_mul: 37,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 23,
            m_table_mul: 93,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 145,
            m_table_mul: 165,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 145,
            m_table_mul: 165,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 101,
            m_table_mul: 37,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 31,
            m_table_mul: 93,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 86,
            m_table_mul: 254,
            m_trans: 4012,
        },
        Etc1GToEac {
            m_base: 153,
            m_table_mul: 165,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 109,
            m_table_mul: 37,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 39,
            m_table_mul: 93,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 163,
            m_table_mul: 165,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 162,
            m_table_mul: 165,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 118,
            m_table_mul: 37,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 48,
            m_table_mul: 93,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 171,
            m_table_mul: 165,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 170,
            m_table_mul: 165,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 126,
            m_table_mul: 37,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 56,
            m_table_mul: 93,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 179,
            m_table_mul: 165,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 178,
            m_table_mul: 165,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 134,
            m_table_mul: 37,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 64,
            m_table_mul: 93,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 187,
            m_table_mul: 165,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 187,
            m_table_mul: 165,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 142,
            m_table_mul: 37,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 72,
            m_table_mul: 93,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 196,
            m_table_mul: 165,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 196,
            m_table_mul: 165,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 151,
            m_table_mul: 37,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 81,
            m_table_mul: 93,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 204,
            m_table_mul: 165,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 204,
            m_table_mul: 165,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 159,
            m_table_mul: 37,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 89,
            m_table_mul: 93,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 212,
            m_table_mul: 165,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 136,
            m_table_mul: 77,
            m_trans: 4008,
        },
        Etc1GToEac {
            m_base: 167,
            m_table_mul: 37,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 97,
            m_table_mul: 93,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 220,
            m_table_mul: 165,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 131,
            m_table_mul: 93,
            m_trans: 4008,
        },
        Etc1GToEac {
            m_base: 175,
            m_table_mul: 37,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 105,
            m_table_mul: 93,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 214,
            m_table_mul: 181,
            m_trans: 4001,
        },
        Etc1GToEac {
            m_base: 140,
            m_table_mul: 93,
            m_trans: 4008,
        },
        Etc1GToEac {
            m_base: 184,
            m_table_mul: 37,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 114,
            m_table_mul: 93,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 222,
            m_table_mul: 181,
            m_trans: 4001,
        },
        Etc1GToEac {
            m_base: 148,
            m_table_mul: 93,
            m_trans: 4008,
        },
        Etc1GToEac {
            m_base: 192,
            m_table_mul: 37,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 122,
            m_table_mul: 93,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 115,
            m_table_mul: 95,
            m_trans: 4085,
        },
        Etc1GToEac {
            m_base: 99,
            m_table_mul: 31,
            m_trans: 4080,
        },
        Etc1GToEac {
            m_base: 115,
            m_table_mul: 95,
            m_trans: 501,
        },
        Etc1GToEac {
            m_base: 99,
            m_table_mul: 31,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 123,
            m_table_mul: 95,
            m_trans: 4085,
        },
        Etc1GToEac {
            m_base: 107,
            m_table_mul: 31,
            m_trans: 4080,
        },
        Etc1GToEac {
            m_base: 123,
            m_table_mul: 95,
            m_trans: 501,
        },
        Etc1GToEac {
            m_base: 107,
            m_table_mul: 31,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 102,
            m_trans: 3840,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 102,
            m_trans: 3840,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 18,
            m_trans: 384,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 18,
            m_trans: 384,
        },
    ],
    [
        Etc1GToEac {
            m_base: 5,
            m_table_mul: 167,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 5,
            m_table_mul: 167,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 13,
            m_trans: 256,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 13,
            m_trans: 256,
        },
    ],
    [
        Etc1GToEac {
            m_base: 4,
            m_table_mul: 54,
            m_trans: 3968,
        },
        Etc1GToEac {
            m_base: 4,
            m_table_mul: 54,
            m_trans: 3968,
        },
        Etc1GToEac {
            m_base: 1,
            m_table_mul: 67,
            m_trans: 448,
        },
        Etc1GToEac {
            m_base: 1,
            m_table_mul: 67,
            m_trans: 448,
        },
    ],
    [
        Etc1GToEac {
            m_base: 30,
            m_table_mul: 198,
            m_trans: 3850,
        },
        Etc1GToEac {
            m_base: 30,
            m_table_mul: 198,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 3,
            m_trans: 480,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 3,
            m_trans: 480,
        },
    ],
    [
        Etc1GToEac {
            m_base: 39,
            m_table_mul: 198,
            m_trans: 3850,
        },
        Etc1GToEac {
            m_base: 39,
            m_table_mul: 198,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 3,
            m_table_mul: 52,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 3,
            m_table_mul: 52,
            m_trans: 488,
        },
    ],
    [
        Etc1GToEac {
            m_base: 47,
            m_table_mul: 198,
            m_trans: 3851,
        },
        Etc1GToEac {
            m_base: 47,
            m_table_mul: 198,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 3,
            m_table_mul: 4,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 3,
            m_table_mul: 4,
            m_trans: 488,
        },
    ],
    [
        Etc1GToEac {
            m_base: 55,
            m_table_mul: 198,
            m_trans: 3851,
        },
        Etc1GToEac {
            m_base: 55,
            m_table_mul: 198,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 1,
            m_table_mul: 70,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 1,
            m_table_mul: 70,
            m_trans: 488,
        },
    ],
    [
        Etc1GToEac {
            m_base: 53,
            m_table_mul: 167,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 63,
            m_table_mul: 198,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 3,
            m_table_mul: 22,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 3,
            m_table_mul: 22,
            m_trans: 488,
        },
    ],
    [
        Etc1GToEac {
            m_base: 62,
            m_table_mul: 167,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 72,
            m_table_mul: 198,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 24,
            m_table_mul: 118,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 6,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 70,
            m_table_mul: 167,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 80,
            m_table_mul: 198,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 32,
            m_table_mul: 118,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 2,
            m_table_mul: 89,
            m_trans: 488,
        },
    ],
    [
        Etc1GToEac {
            m_base: 78,
            m_table_mul: 167,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 88,
            m_table_mul: 198,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 40,
            m_table_mul: 118,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 1,
            m_table_mul: 73,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 86,
            m_table_mul: 167,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 96,
            m_table_mul: 198,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 48,
            m_table_mul: 118,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 28,
            m_trans: 424,
        },
    ],
    [
        Etc1GToEac {
            m_base: 95,
            m_table_mul: 167,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 105,
            m_table_mul: 198,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 57,
            m_table_mul: 118,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 9,
            m_table_mul: 28,
            m_trans: 424,
        },
    ],
    [
        Etc1GToEac {
            m_base: 103,
            m_table_mul: 167,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 113,
            m_table_mul: 198,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 65,
            m_table_mul: 118,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 5,
            m_table_mul: 108,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 111,
            m_table_mul: 167,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 121,
            m_table_mul: 198,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 73,
            m_table_mul: 118,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 13,
            m_table_mul: 108,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 119,
            m_table_mul: 167,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 129,
            m_table_mul: 198,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 81,
            m_table_mul: 118,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 21,
            m_table_mul: 108,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 128,
            m_table_mul: 167,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 138,
            m_table_mul: 198,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 90,
            m_table_mul: 118,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 6,
            m_table_mul: 28,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 136,
            m_table_mul: 167,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 146,
            m_table_mul: 198,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 98,
            m_table_mul: 118,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 14,
            m_table_mul: 28,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 145,
            m_table_mul: 167,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 154,
            m_table_mul: 198,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 106,
            m_table_mul: 118,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 22,
            m_table_mul: 28,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 153,
            m_table_mul: 167,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 162,
            m_table_mul: 198,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 114,
            m_table_mul: 118,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 30,
            m_table_mul: 28,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 162,
            m_table_mul: 167,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 171,
            m_table_mul: 198,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 123,
            m_table_mul: 118,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 39,
            m_table_mul: 28,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 170,
            m_table_mul: 167,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 179,
            m_table_mul: 198,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 131,
            m_table_mul: 118,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 47,
            m_table_mul: 28,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 178,
            m_table_mul: 167,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 187,
            m_table_mul: 198,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 139,
            m_table_mul: 118,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 55,
            m_table_mul: 28,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 186,
            m_table_mul: 167,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 195,
            m_table_mul: 198,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 147,
            m_table_mul: 118,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 63,
            m_table_mul: 28,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 194,
            m_table_mul: 167,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 120,
            m_table_mul: 12,
            m_trans: 4008,
        },
        Etc1GToEac {
            m_base: 156,
            m_table_mul: 118,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 72,
            m_table_mul: 28,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 206,
            m_table_mul: 198,
            m_trans: 3907,
        },
        Etc1GToEac {
            m_base: 116,
            m_table_mul: 28,
            m_trans: 4008,
        },
        Etc1GToEac {
            m_base: 164,
            m_table_mul: 118,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 80,
            m_table_mul: 28,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 214,
            m_table_mul: 198,
            m_trans: 3907,
        },
        Etc1GToEac {
            m_base: 124,
            m_table_mul: 28,
            m_trans: 4008,
        },
        Etc1GToEac {
            m_base: 172,
            m_table_mul: 118,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 88,
            m_table_mul: 28,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 222,
            m_table_mul: 198,
            m_trans: 3395,
        },
        Etc1GToEac {
            m_base: 132,
            m_table_mul: 28,
            m_trans: 4008,
        },
        Etc1GToEac {
            m_base: 180,
            m_table_mul: 118,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 96,
            m_table_mul: 28,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 207,
            m_table_mul: 134,
            m_trans: 4001,
        },
        Etc1GToEac {
            m_base: 141,
            m_table_mul: 28,
            m_trans: 4008,
        },
        Etc1GToEac {
            m_base: 189,
            m_table_mul: 118,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 105,
            m_table_mul: 28,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 95,
            m_table_mul: 30,
            m_trans: 4085,
        },
        Etc1GToEac {
            m_base: 86,
            m_table_mul: 31,
            m_trans: 4080,
        },
        Etc1GToEac {
            m_base: 95,
            m_table_mul: 30,
            m_trans: 501,
        },
        Etc1GToEac {
            m_base: 86,
            m_table_mul: 31,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 103,
            m_table_mul: 30,
            m_trans: 4085,
        },
        Etc1GToEac {
            m_base: 94,
            m_table_mul: 31,
            m_trans: 4080,
        },
        Etc1GToEac {
            m_base: 103,
            m_table_mul: 30,
            m_trans: 501,
        },
        Etc1GToEac {
            m_base: 94,
            m_table_mul: 31,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 111,
            m_table_mul: 30,
            m_trans: 4085,
        },
        Etc1GToEac {
            m_base: 102,
            m_table_mul: 31,
            m_trans: 4080,
        },
        Etc1GToEac {
            m_base: 111,
            m_table_mul: 30,
            m_trans: 501,
        },
        Etc1GToEac {
            m_base: 102,
            m_table_mul: 31,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 104,
            m_trans: 3840,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 104,
            m_trans: 3840,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 18,
            m_trans: 448,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 18,
            m_trans: 448,
        },
    ],
    [
        Etc1GToEac {
            m_base: 4,
            m_table_mul: 39,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 4,
            m_table_mul: 39,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 4,
            m_trans: 384,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 4,
            m_trans: 384,
        },
    ],
    [
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 56,
            m_trans: 3968,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 56,
            m_trans: 3968,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 84,
            m_trans: 448,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 84,
            m_trans: 448,
        },
    ],
    [
        Etc1GToEac {
            m_base: 6,
            m_table_mul: 110,
            m_trans: 3328,
        },
        Etc1GToEac {
            m_base: 6,
            m_table_mul: 110,
            m_trans: 3328,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 20,
            m_trans: 448,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 20,
            m_trans: 448,
        },
    ],
    [
        Etc1GToEac {
            m_base: 41,
            m_table_mul: 200,
            m_trans: 3850,
        },
        Etc1GToEac {
            m_base: 41,
            m_table_mul: 200,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 1,
            m_table_mul: 4,
            m_trans: 480,
        },
        Etc1GToEac {
            m_base: 1,
            m_table_mul: 4,
            m_trans: 480,
        },
    ],
    [
        Etc1GToEac {
            m_base: 49,
            m_table_mul: 200,
            m_trans: 3850,
        },
        Etc1GToEac {
            m_base: 49,
            m_table_mul: 200,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 1,
            m_table_mul: 8,
            m_trans: 416,
        },
        Etc1GToEac {
            m_base: 1,
            m_table_mul: 8,
            m_trans: 416,
        },
    ],
    [
        Etc1GToEac {
            m_base: 57,
            m_table_mul: 200,
            m_trans: 3851,
        },
        Etc1GToEac {
            m_base: 57,
            m_table_mul: 200,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 1,
            m_table_mul: 38,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 1,
            m_table_mul: 38,
            m_trans: 488,
        },
    ],
    [
        Etc1GToEac {
            m_base: 65,
            m_table_mul: 200,
            m_trans: 3851,
        },
        Etc1GToEac {
            m_base: 65,
            m_table_mul: 200,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 1,
            m_table_mul: 120,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 1,
            m_table_mul: 120,
            m_trans: 488,
        },
    ],
    [
        Etc1GToEac {
            m_base: 74,
            m_table_mul: 200,
            m_trans: 3851,
        },
        Etc1GToEac {
            m_base: 74,
            m_table_mul: 200,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 2,
            m_table_mul: 72,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 2,
            m_table_mul: 72,
            m_trans: 488,
        },
    ],
    [
        Etc1GToEac {
            m_base: 68,
            m_table_mul: 6,
            m_trans: 3907,
        },
        Etc1GToEac {
            m_base: 82,
            m_table_mul: 200,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 2,
            m_table_mul: 24,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 2,
            m_table_mul: 24,
            m_trans: 488,
        },
    ],
    [
        Etc1GToEac {
            m_base: 77,
            m_table_mul: 6,
            m_trans: 3907,
        },
        Etc1GToEac {
            m_base: 90,
            m_table_mul: 200,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 26,
            m_table_mul: 120,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 10,
            m_table_mul: 24,
            m_trans: 488,
        },
    ],
    [
        Etc1GToEac {
            m_base: 97,
            m_table_mul: 63,
            m_trans: 3330,
        },
        Etc1GToEac {
            m_base: 98,
            m_table_mul: 200,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 34,
            m_table_mul: 120,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 2,
            m_table_mul: 8,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 106,
            m_table_mul: 63,
            m_trans: 3330,
        },
        Etc1GToEac {
            m_base: 107,
            m_table_mul: 200,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 43,
            m_table_mul: 120,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 3,
            m_table_mul: 92,
            m_trans: 488,
        },
    ],
    [
        Etc1GToEac {
            m_base: 114,
            m_table_mul: 63,
            m_trans: 3330,
        },
        Etc1GToEac {
            m_base: 115,
            m_table_mul: 200,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 51,
            m_table_mul: 120,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 11,
            m_table_mul: 92,
            m_trans: 488,
        },
    ],
    [
        Etc1GToEac {
            m_base: 122,
            m_table_mul: 63,
            m_trans: 3330,
        },
        Etc1GToEac {
            m_base: 123,
            m_table_mul: 200,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 59,
            m_table_mul: 120,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 7,
            m_table_mul: 76,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 130,
            m_table_mul: 63,
            m_trans: 3330,
        },
        Etc1GToEac {
            m_base: 131,
            m_table_mul: 200,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 67,
            m_table_mul: 120,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 15,
            m_table_mul: 76,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 139,
            m_table_mul: 63,
            m_trans: 3330,
        },
        Etc1GToEac {
            m_base: 140,
            m_table_mul: 200,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 76,
            m_table_mul: 120,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 24,
            m_table_mul: 76,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 147,
            m_table_mul: 63,
            m_trans: 3330,
        },
        Etc1GToEac {
            m_base: 148,
            m_table_mul: 200,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 84,
            m_table_mul: 120,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 32,
            m_table_mul: 76,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 155,
            m_table_mul: 63,
            m_trans: 3330,
        },
        Etc1GToEac {
            m_base: 156,
            m_table_mul: 200,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 92,
            m_table_mul: 120,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 40,
            m_table_mul: 76,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 164,
            m_table_mul: 63,
            m_trans: 3330,
        },
        Etc1GToEac {
            m_base: 164,
            m_table_mul: 200,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 100,
            m_table_mul: 120,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 48,
            m_table_mul: 76,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 173,
            m_table_mul: 63,
            m_trans: 3330,
        },
        Etc1GToEac {
            m_base: 173,
            m_table_mul: 200,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 109,
            m_table_mul: 120,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 57,
            m_table_mul: 76,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 184,
            m_table_mul: 6,
            m_trans: 3851,
        },
        Etc1GToEac {
            m_base: 181,
            m_table_mul: 200,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 117,
            m_table_mul: 120,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 65,
            m_table_mul: 76,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 192,
            m_table_mul: 6,
            m_trans: 3851,
        },
        Etc1GToEac {
            m_base: 133,
            m_table_mul: 28,
            m_trans: 3936,
        },
        Etc1GToEac {
            m_base: 125,
            m_table_mul: 120,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 73,
            m_table_mul: 76,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 189,
            m_table_mul: 200,
            m_trans: 3907,
        },
        Etc1GToEac {
            m_base: 141,
            m_table_mul: 28,
            m_trans: 3936,
        },
        Etc1GToEac {
            m_base: 133,
            m_table_mul: 120,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 81,
            m_table_mul: 76,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 198,
            m_table_mul: 200,
            m_trans: 3907,
        },
        Etc1GToEac {
            m_base: 138,
            m_table_mul: 108,
            m_trans: 4000,
        },
        Etc1GToEac {
            m_base: 142,
            m_table_mul: 120,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 90,
            m_table_mul: 76,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 206,
            m_table_mul: 200,
            m_trans: 3907,
        },
        Etc1GToEac {
            m_base: 146,
            m_table_mul: 108,
            m_trans: 4000,
        },
        Etc1GToEac {
            m_base: 150,
            m_table_mul: 120,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 98,
            m_table_mul: 76,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 214,
            m_table_mul: 200,
            m_trans: 3395,
        },
        Etc1GToEac {
            m_base: 154,
            m_table_mul: 108,
            m_trans: 4000,
        },
        Etc1GToEac {
            m_base: 158,
            m_table_mul: 120,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 106,
            m_table_mul: 76,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 190,
            m_table_mul: 136,
            m_trans: 4001,
        },
        Etc1GToEac {
            m_base: 162,
            m_table_mul: 108,
            m_trans: 4000,
        },
        Etc1GToEac {
            m_base: 166,
            m_table_mul: 120,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 114,
            m_table_mul: 76,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 123,
            m_table_mul: 30,
            m_trans: 4076,
        },
        Etc1GToEac {
            m_base: 87,
            m_table_mul: 15,
            m_trans: 4080,
        },
        Etc1GToEac {
            m_base: 123,
            m_table_mul: 30,
            m_trans: 492,
        },
        Etc1GToEac {
            m_base: 87,
            m_table_mul: 15,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 117,
            m_table_mul: 110,
            m_trans: 4084,
        },
        Etc1GToEac {
            m_base: 80,
            m_table_mul: 31,
            m_trans: 4080,
        },
        Etc1GToEac {
            m_base: 117,
            m_table_mul: 110,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 80,
            m_table_mul: 31,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 125,
            m_table_mul: 110,
            m_trans: 4084,
        },
        Etc1GToEac {
            m_base: 88,
            m_table_mul: 31,
            m_trans: 4080,
        },
        Etc1GToEac {
            m_base: 125,
            m_table_mul: 110,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 88,
            m_table_mul: 31,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 133,
            m_table_mul: 110,
            m_trans: 4084,
        },
        Etc1GToEac {
            m_base: 96,
            m_table_mul: 31,
            m_trans: 4080,
        },
        Etc1GToEac {
            m_base: 133,
            m_table_mul: 110,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 96,
            m_table_mul: 31,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 9,
            m_table_mul: 56,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 9,
            m_table_mul: 56,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 67,
            m_trans: 448,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 67,
            m_trans: 448,
        },
    ],
    [
        Etc1GToEac {
            m_base: 1,
            m_table_mul: 8,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 1,
            m_table_mul: 8,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 1,
            m_table_mul: 84,
            m_trans: 448,
        },
        Etc1GToEac {
            m_base: 1,
            m_table_mul: 84,
            m_trans: 448,
        },
    ],
    [
        Etc1GToEac {
            m_base: 1,
            m_table_mul: 124,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 1,
            m_table_mul: 124,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 39,
            m_trans: 384,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 39,
            m_trans: 384,
        },
    ],
    [
        Etc1GToEac {
            m_base: 9,
            m_table_mul: 124,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 9,
            m_table_mul: 124,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 1,
            m_table_mul: 4,
            m_trans: 448,
        },
        Etc1GToEac {
            m_base: 1,
            m_table_mul: 4,
            m_trans: 448,
        },
    ],
    [
        Etc1GToEac {
            m_base: 6,
            m_table_mul: 76,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 6,
            m_table_mul: 76,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 70,
            m_trans: 448,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 70,
            m_trans: 448,
        },
    ],
    [
        Etc1GToEac {
            m_base: 62,
            m_table_mul: 6,
            m_trans: 3859,
        },
        Etc1GToEac {
            m_base: 62,
            m_table_mul: 6,
            m_trans: 3856,
        },
        Etc1GToEac {
            m_base: 2,
            m_table_mul: 38,
            m_trans: 480,
        },
        Etc1GToEac {
            m_base: 2,
            m_table_mul: 38,
            m_trans: 480,
        },
    ],
    [
        Etc1GToEac {
            m_base: 70,
            m_table_mul: 6,
            m_trans: 3859,
        },
        Etc1GToEac {
            m_base: 70,
            m_table_mul: 6,
            m_trans: 3856,
        },
        Etc1GToEac {
            m_base: 5,
            m_table_mul: 43,
            m_trans: 416,
        },
        Etc1GToEac {
            m_base: 5,
            m_table_mul: 43,
            m_trans: 416,
        },
    ],
    [
        Etc1GToEac {
            m_base: 78,
            m_table_mul: 6,
            m_trans: 3859,
        },
        Etc1GToEac {
            m_base: 78,
            m_table_mul: 6,
            m_trans: 3856,
        },
        Etc1GToEac {
            m_base: 2,
            m_table_mul: 11,
            m_trans: 416,
        },
        Etc1GToEac {
            m_base: 2,
            m_table_mul: 11,
            m_trans: 416,
        },
    ],
    [
        Etc1GToEac {
            m_base: 87,
            m_table_mul: 6,
            m_trans: 3859,
        },
        Etc1GToEac {
            m_base: 87,
            m_table_mul: 6,
            m_trans: 3856,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 171,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 171,
            m_trans: 488,
        },
    ],
    [
        Etc1GToEac {
            m_base: 67,
            m_table_mul: 8,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 95,
            m_table_mul: 6,
            m_trans: 3856,
        },
        Etc1GToEac {
            m_base: 8,
            m_table_mul: 171,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 8,
            m_table_mul: 171,
            m_trans: 488,
        },
    ],
    [
        Etc1GToEac {
            m_base: 75,
            m_table_mul: 8,
            m_trans: 3907,
        },
        Etc1GToEac {
            m_base: 103,
            m_table_mul: 6,
            m_trans: 3856,
        },
        Etc1GToEac {
            m_base: 5,
            m_table_mul: 123,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 5,
            m_table_mul: 123,
            m_trans: 488,
        },
    ],
    [
        Etc1GToEac {
            m_base: 83,
            m_table_mul: 8,
            m_trans: 3907,
        },
        Etc1GToEac {
            m_base: 111,
            m_table_mul: 6,
            m_trans: 3856,
        },
        Etc1GToEac {
            m_base: 2,
            m_table_mul: 75,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 2,
            m_table_mul: 75,
            m_trans: 488,
        },
    ],
    [
        Etc1GToEac {
            m_base: 92,
            m_table_mul: 8,
            m_trans: 3907,
        },
        Etc1GToEac {
            m_base: 120,
            m_table_mul: 6,
            m_trans: 3856,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 27,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 27,
            m_trans: 488,
        },
    ],
    [
        Etc1GToEac {
            m_base: 100,
            m_table_mul: 8,
            m_trans: 3907,
        },
        Etc1GToEac {
            m_base: 128,
            m_table_mul: 6,
            m_trans: 3856,
        },
        Etc1GToEac {
            m_base: 8,
            m_table_mul: 27,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 8,
            m_table_mul: 27,
            m_trans: 488,
        },
    ],
    [
        Etc1GToEac {
            m_base: 120,
            m_table_mul: 106,
            m_trans: 3843,
        },
        Etc1GToEac {
            m_base: 136,
            m_table_mul: 6,
            m_trans: 3856,
        },
        Etc1GToEac {
            m_base: 99,
            m_table_mul: 6,
            m_trans: 387,
        },
        Etc1GToEac {
            m_base: 16,
            m_table_mul: 27,
            m_trans: 488,
        },
    ],
    [
        Etc1GToEac {
            m_base: 128,
            m_table_mul: 106,
            m_trans: 3843,
        },
        Etc1GToEac {
            m_base: 144,
            m_table_mul: 6,
            m_trans: 3856,
        },
        Etc1GToEac {
            m_base: 107,
            m_table_mul: 6,
            m_trans: 387,
        },
        Etc1GToEac {
            m_base: 2,
            m_table_mul: 11,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 137,
            m_table_mul: 106,
            m_trans: 3843,
        },
        Etc1GToEac {
            m_base: 153,
            m_table_mul: 6,
            m_trans: 3856,
        },
        Etc1GToEac {
            m_base: 117,
            m_table_mul: 6,
            m_trans: 387,
        },
        Etc1GToEac {
            m_base: 11,
            m_table_mul: 11,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 145,
            m_table_mul: 106,
            m_trans: 3843,
        },
        Etc1GToEac {
            m_base: 161,
            m_table_mul: 6,
            m_trans: 3856,
        },
        Etc1GToEac {
            m_base: 125,
            m_table_mul: 6,
            m_trans: 387,
        },
        Etc1GToEac {
            m_base: 19,
            m_table_mul: 11,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 163,
            m_table_mul: 8,
            m_trans: 3851,
        },
        Etc1GToEac {
            m_base: 137,
            m_table_mul: 43,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 133,
            m_table_mul: 6,
            m_trans: 387,
        },
        Etc1GToEac {
            m_base: 27,
            m_table_mul: 11,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 171,
            m_table_mul: 8,
            m_trans: 3851,
        },
        Etc1GToEac {
            m_base: 145,
            m_table_mul: 43,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 141,
            m_table_mul: 6,
            m_trans: 387,
        },
        Etc1GToEac {
            m_base: 35,
            m_table_mul: 11,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 180,
            m_table_mul: 8,
            m_trans: 3851,
        },
        Etc1GToEac {
            m_base: 110,
            m_table_mul: 11,
            m_trans: 4000,
        },
        Etc1GToEac {
            m_base: 150,
            m_table_mul: 6,
            m_trans: 387,
        },
        Etc1GToEac {
            m_base: 44,
            m_table_mul: 11,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 188,
            m_table_mul: 8,
            m_trans: 3851,
        },
        Etc1GToEac {
            m_base: 118,
            m_table_mul: 11,
            m_trans: 4000,
        },
        Etc1GToEac {
            m_base: 158,
            m_table_mul: 6,
            m_trans: 387,
        },
        Etc1GToEac {
            m_base: 52,
            m_table_mul: 11,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 172,
            m_table_mul: 72,
            m_trans: 3907,
        },
        Etc1GToEac {
            m_base: 126,
            m_table_mul: 11,
            m_trans: 4000,
        },
        Etc1GToEac {
            m_base: 166,
            m_table_mul: 6,
            m_trans: 387,
        },
        Etc1GToEac {
            m_base: 60,
            m_table_mul: 11,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 174,
            m_table_mul: 6,
            m_trans: 3971,
        },
        Etc1GToEac {
            m_base: 134,
            m_table_mul: 11,
            m_trans: 4000,
        },
        Etc1GToEac {
            m_base: 174,
            m_table_mul: 6,
            m_trans: 387,
        },
        Etc1GToEac {
            m_base: 68,
            m_table_mul: 11,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 183,
            m_table_mul: 6,
            m_trans: 3971,
        },
        Etc1GToEac {
            m_base: 143,
            m_table_mul: 11,
            m_trans: 4000,
        },
        Etc1GToEac {
            m_base: 183,
            m_table_mul: 6,
            m_trans: 387,
        },
        Etc1GToEac {
            m_base: 77,
            m_table_mul: 11,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 191,
            m_table_mul: 6,
            m_trans: 3971,
        },
        Etc1GToEac {
            m_base: 151,
            m_table_mul: 11,
            m_trans: 4000,
        },
        Etc1GToEac {
            m_base: 191,
            m_table_mul: 6,
            m_trans: 387,
        },
        Etc1GToEac {
            m_base: 85,
            m_table_mul: 11,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 199,
            m_table_mul: 6,
            m_trans: 3971,
        },
        Etc1GToEac {
            m_base: 159,
            m_table_mul: 11,
            m_trans: 4000,
        },
        Etc1GToEac {
            m_base: 199,
            m_table_mul: 6,
            m_trans: 387,
        },
        Etc1GToEac {
            m_base: 93,
            m_table_mul: 11,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 92,
            m_table_mul: 12,
            m_trans: 4084,
        },
        Etc1GToEac {
            m_base: 69,
            m_table_mul: 15,
            m_trans: 4080,
        },
        Etc1GToEac {
            m_base: 92,
            m_table_mul: 12,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 69,
            m_table_mul: 15,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 101,
            m_table_mul: 12,
            m_trans: 4084,
        },
        Etc1GToEac {
            m_base: 78,
            m_table_mul: 15,
            m_trans: 4080,
        },
        Etc1GToEac {
            m_base: 101,
            m_table_mul: 12,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 78,
            m_table_mul: 15,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 110,
            m_table_mul: 12,
            m_trans: 4084,
        },
        Etc1GToEac {
            m_base: 86,
            m_table_mul: 15,
            m_trans: 4080,
        },
        Etc1GToEac {
            m_base: 110,
            m_table_mul: 12,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 86,
            m_table_mul: 15,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 118,
            m_table_mul: 12,
            m_trans: 4084,
        },
        Etc1GToEac {
            m_base: 79,
            m_table_mul: 31,
            m_trans: 4080,
        },
        Etc1GToEac {
            m_base: 118,
            m_table_mul: 12,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 79,
            m_table_mul: 31,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 126,
            m_table_mul: 12,
            m_trans: 4084,
        },
        Etc1GToEac {
            m_base: 87,
            m_table_mul: 31,
            m_trans: 4080,
        },
        Etc1GToEac {
            m_base: 126,
            m_table_mul: 12,
            m_trans: 500,
        },
        Etc1GToEac {
            m_base: 87,
            m_table_mul: 31,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 71,
            m_table_mul: 8,
            m_trans: 3602,
        },
        Etc1GToEac {
            m_base: 71,
            m_table_mul: 8,
            m_trans: 3600,
        },
        Etc1GToEac {
            m_base: 2,
            m_table_mul: 21,
            m_trans: 384,
        },
        Etc1GToEac {
            m_base: 2,
            m_table_mul: 21,
            m_trans: 384,
        },
    ],
    [
        Etc1GToEac {
            m_base: 79,
            m_table_mul: 8,
            m_trans: 3611,
        },
        Etc1GToEac {
            m_base: 79,
            m_table_mul: 8,
            m_trans: 3608,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 69,
            m_trans: 448,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 69,
            m_trans: 448,
        },
    ],
    [
        Etc1GToEac {
            m_base: 87,
            m_table_mul: 8,
            m_trans: 3611,
        },
        Etc1GToEac {
            m_base: 87,
            m_table_mul: 8,
            m_trans: 3608,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 23,
            m_trans: 384,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 23,
            m_trans: 384,
        },
    ],
    [
        Etc1GToEac {
            m_base: 95,
            m_table_mul: 8,
            m_trans: 3611,
        },
        Etc1GToEac {
            m_base: 95,
            m_table_mul: 8,
            m_trans: 3608,
        },
        Etc1GToEac {
            m_base: 1,
            m_table_mul: 5,
            m_trans: 448,
        },
        Etc1GToEac {
            m_base: 1,
            m_table_mul: 5,
            m_trans: 448,
        },
    ],
    [
        Etc1GToEac {
            m_base: 104,
            m_table_mul: 8,
            m_trans: 3611,
        },
        Etc1GToEac {
            m_base: 104,
            m_table_mul: 8,
            m_trans: 3608,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 88,
            m_trans: 448,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 88,
            m_trans: 448,
        },
    ],
    [
        Etc1GToEac {
            m_base: 112,
            m_table_mul: 8,
            m_trans: 3611,
        },
        Etc1GToEac {
            m_base: 112,
            m_table_mul: 8,
            m_trans: 3608,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 72,
            m_trans: 448,
        },
        Etc1GToEac {
            m_base: 0,
            m_table_mul: 72,
            m_trans: 448,
        },
    ],
    [
        Etc1GToEac {
            m_base: 120,
            m_table_mul: 8,
            m_trans: 3611,
        },
        Etc1GToEac {
            m_base: 121,
            m_table_mul: 8,
            m_trans: 3608,
        },
        Etc1GToEac {
            m_base: 36,
            m_table_mul: 21,
            m_trans: 458,
        },
        Etc1GToEac {
            m_base: 36,
            m_table_mul: 21,
            m_trans: 456,
        },
    ],
    [
        Etc1GToEac {
            m_base: 133,
            m_table_mul: 47,
            m_trans: 3091,
        },
        Etc1GToEac {
            m_base: 129,
            m_table_mul: 8,
            m_trans: 3608,
        },
        Etc1GToEac {
            m_base: 44,
            m_table_mul: 21,
            m_trans: 458,
        },
        Etc1GToEac {
            m_base: 44,
            m_table_mul: 21,
            m_trans: 456,
        },
    ],
    [
        Etc1GToEac {
            m_base: 142,
            m_table_mul: 47,
            m_trans: 3091,
        },
        Etc1GToEac {
            m_base: 138,
            m_table_mul: 8,
            m_trans: 3608,
        },
        Etc1GToEac {
            m_base: 53,
            m_table_mul: 21,
            m_trans: 459,
        },
        Etc1GToEac {
            m_base: 53,
            m_table_mul: 21,
            m_trans: 456,
        },
    ],
    [
        Etc1GToEac {
            m_base: 98,
            m_table_mul: 12,
            m_trans: 3850,
        },
        Etc1GToEac {
            m_base: 98,
            m_table_mul: 12,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 61,
            m_table_mul: 21,
            m_trans: 459,
        },
        Etc1GToEac {
            m_base: 61,
            m_table_mul: 21,
            m_trans: 456,
        },
    ],
    [
        Etc1GToEac {
            m_base: 106,
            m_table_mul: 12,
            m_trans: 3850,
        },
        Etc1GToEac {
            m_base: 106,
            m_table_mul: 12,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 10,
            m_table_mul: 92,
            m_trans: 480,
        },
        Etc1GToEac {
            m_base: 69,
            m_table_mul: 21,
            m_trans: 456,
        },
    ],
    [
        Etc1GToEac {
            m_base: 114,
            m_table_mul: 12,
            m_trans: 3851,
        },
        Etc1GToEac {
            m_base: 114,
            m_table_mul: 12,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 18,
            m_table_mul: 92,
            m_trans: 480,
        },
        Etc1GToEac {
            m_base: 77,
            m_table_mul: 21,
            m_trans: 456,
        },
    ],
    [
        Etc1GToEac {
            m_base: 123,
            m_table_mul: 12,
            m_trans: 3851,
        },
        Etc1GToEac {
            m_base: 123,
            m_table_mul: 12,
            m_trans: 3848,
        },
        Etc1GToEac {
            m_base: 3,
            m_table_mul: 44,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 86,
            m_table_mul: 21,
            m_trans: 456,
        },
    ],
    [
        Etc1GToEac {
            m_base: 95,
            m_table_mul: 12,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 95,
            m_table_mul: 12,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 11,
            m_table_mul: 44,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 94,
            m_table_mul: 21,
            m_trans: 456,
        },
    ],
    [
        Etc1GToEac {
            m_base: 103,
            m_table_mul: 12,
            m_trans: 3906,
        },
        Etc1GToEac {
            m_base: 103,
            m_table_mul: 12,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 19,
            m_table_mul: 44,
            m_trans: 488,
        },
        Etc1GToEac {
            m_base: 102,
            m_table_mul: 21,
            m_trans: 456,
        },
    ],
    [
        Etc1GToEac {
            m_base: 111,
            m_table_mul: 12,
            m_trans: 3907,
        },
        Etc1GToEac {
            m_base: 111,
            m_table_mul: 12,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 27,
            m_table_mul: 44,
            m_trans: 489,
        },
        Etc1GToEac {
            m_base: 110,
            m_table_mul: 21,
            m_trans: 456,
        },
    ],
    [
        Etc1GToEac {
            m_base: 120,
            m_table_mul: 12,
            m_trans: 3907,
        },
        Etc1GToEac {
            m_base: 120,
            m_table_mul: 12,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 36,
            m_table_mul: 44,
            m_trans: 489,
        },
        Etc1GToEac {
            m_base: 119,
            m_table_mul: 21,
            m_trans: 456,
        },
    ],
    [
        Etc1GToEac {
            m_base: 128,
            m_table_mul: 12,
            m_trans: 3907,
        },
        Etc1GToEac {
            m_base: 128,
            m_table_mul: 12,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 44,
            m_table_mul: 44,
            m_trans: 489,
        },
        Etc1GToEac {
            m_base: 127,
            m_table_mul: 21,
            m_trans: 456,
        },
    ],
    [
        Etc1GToEac {
            m_base: 136,
            m_table_mul: 12,
            m_trans: 3907,
        },
        Etc1GToEac {
            m_base: 136,
            m_table_mul: 12,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 52,
            m_table_mul: 44,
            m_trans: 489,
        },
        Etc1GToEac {
            m_base: 135,
            m_table_mul: 21,
            m_trans: 456,
        },
    ],
    [
        Etc1GToEac {
            m_base: 144,
            m_table_mul: 12,
            m_trans: 3907,
        },
        Etc1GToEac {
            m_base: 144,
            m_table_mul: 12,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 60,
            m_table_mul: 44,
            m_trans: 490,
        },
        Etc1GToEac {
            m_base: 144,
            m_table_mul: 21,
            m_trans: 456,
        },
    ],
    [
        Etc1GToEac {
            m_base: 153,
            m_table_mul: 12,
            m_trans: 3907,
        },
        Etc1GToEac {
            m_base: 153,
            m_table_mul: 12,
            m_trans: 3904,
        },
        Etc1GToEac {
            m_base: 69,
            m_table_mul: 44,
            m_trans: 490,
        },
        Etc1GToEac {
            m_base: 153,
            m_table_mul: 21,
            m_trans: 456,
        },
    ],
    [
        Etc1GToEac {
            m_base: 161,
            m_table_mul: 12,
            m_trans: 3395,
        },
        Etc1GToEac {
            m_base: 149,
            m_table_mul: 188,
            m_trans: 3968,
        },
        Etc1GToEac {
            m_base: 77,
            m_table_mul: 44,
            m_trans: 490,
        },
        Etc1GToEac {
            m_base: 161,
            m_table_mul: 21,
            m_trans: 456,
        },
    ],
    [
        Etc1GToEac {
            m_base: 169,
            m_table_mul: 12,
            m_trans: 3395,
        },
        Etc1GToEac {
            m_base: 199,
            m_table_mul: 21,
            m_trans: 3928,
        },
        Etc1GToEac {
            m_base: 85,
            m_table_mul: 44,
            m_trans: 490,
        },
        Etc1GToEac {
            m_base: 169,
            m_table_mul: 21,
            m_trans: 456,
        },
    ],
    [
        Etc1GToEac {
            m_base: 113,
            m_table_mul: 95,
            m_trans: 4001,
        },
        Etc1GToEac {
            m_base: 202,
            m_table_mul: 69,
            m_trans: 3992,
        },
        Etc1GToEac {
            m_base: 125,
            m_table_mul: 8,
            m_trans: 483,
        },
        Etc1GToEac {
            m_base: 177,
            m_table_mul: 21,
            m_trans: 456,
        },
    ],
    [
        Etc1GToEac {
            m_base: 122,
            m_table_mul: 95,
            m_trans: 4001,
        },
        Etc1GToEac {
            m_base: 201,
            m_table_mul: 21,
            m_trans: 3984,
        },
        Etc1GToEac {
            m_base: 134,
            m_table_mul: 8,
            m_trans: 483,
        },
        Etc1GToEac {
            m_base: 186,
            m_table_mul: 21,
            m_trans: 456,
        },
    ],
    [
        Etc1GToEac {
            m_base: 143,
            m_table_mul: 8,
            m_trans: 4067,
        },
        Etc1GToEac {
            m_base: 209,
            m_table_mul: 21,
            m_trans: 3984,
        },
        Etc1GToEac {
            m_base: 142,
            m_table_mul: 8,
            m_trans: 483,
        },
        Etc1GToEac {
            m_base: 194,
            m_table_mul: 21,
            m_trans: 456,
        },
    ],
    [
        Etc1GToEac {
            m_base: 151,
            m_table_mul: 8,
            m_trans: 4067,
        },
        Etc1GToEac {
            m_base: 47,
            m_table_mul: 15,
            m_trans: 4080,
        },
        Etc1GToEac {
            m_base: 151,
            m_table_mul: 8,
            m_trans: 483,
        },
        Etc1GToEac {
            m_base: 47,
            m_table_mul: 15,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 159,
            m_table_mul: 8,
            m_trans: 4067,
        },
        Etc1GToEac {
            m_base: 55,
            m_table_mul: 15,
            m_trans: 4080,
        },
        Etc1GToEac {
            m_base: 159,
            m_table_mul: 8,
            m_trans: 483,
        },
        Etc1GToEac {
            m_base: 55,
            m_table_mul: 15,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 168,
            m_table_mul: 8,
            m_trans: 4067,
        },
        Etc1GToEac {
            m_base: 64,
            m_table_mul: 15,
            m_trans: 4080,
        },
        Etc1GToEac {
            m_base: 168,
            m_table_mul: 8,
            m_trans: 483,
        },
        Etc1GToEac {
            m_base: 64,
            m_table_mul: 15,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 160,
            m_table_mul: 40,
            m_trans: 4075,
        },
        Etc1GToEac {
            m_base: 72,
            m_table_mul: 15,
            m_trans: 4080,
        },
        Etc1GToEac {
            m_base: 160,
            m_table_mul: 40,
            m_trans: 491,
        },
        Etc1GToEac {
            m_base: 72,
            m_table_mul: 15,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 168,
            m_table_mul: 40,
            m_trans: 4075,
        },
        Etc1GToEac {
            m_base: 80,
            m_table_mul: 15,
            m_trans: 4080,
        },
        Etc1GToEac {
            m_base: 168,
            m_table_mul: 40,
            m_trans: 491,
        },
        Etc1GToEac {
            m_base: 80,
            m_table_mul: 15,
            m_trans: 496,
        },
    ],
    [
        Etc1GToEac {
            m_base: 144,
            m_table_mul: 8,
            m_trans: 4082,
        },
        Etc1GToEac {
            m_base: 88,
            m_table_mul: 15,
            m_trans: 4080,
        },
        Etc1GToEac {
            m_base: 144,
            m_table_mul: 8,
            m_trans: 498,
        },
        Etc1GToEac {
            m_base: 88,
            m_table_mul: 15,
            m_trans: 496,
        },
    ],
];
