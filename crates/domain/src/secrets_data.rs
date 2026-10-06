use crate::secrets::Sealed;

// Sealed with `cargo xtask seal --name DB_EXPORT_HOST`. Plaintext is not stored anywhere.
pub const DB_EXPORT_HOST: Sealed = Sealed {
    ciphertext: &[
        0xA9, 0xC3, 0xDD, 0xD2, 0xF9, 0x9B, 0x72, 0x63, 0xCD, 0xAA, 0x59, 0xB8, 0x63, 0xF9, 0xD7,
        0x59, 0x42, 0x95, 0x3F, 0xA8, 0xFD, 0xF3, 0x03, 0xCE, 0xCD, 0x65, 0xE4, 0x25, 0x7F, 0xE5,
        0x9A, 0x75, 0xBB, 0x86, 0xB9, 0xEC, 0xE9, 0xC9,
    ],
    nonce_b64: "hmr9DPtLmb1Uwt/F",
    table: &[
        (141, 676),
        (134, 406),
        (939, 331),
        (367, 409),
        (262, 274),
        (224, 434),
        (753, 711),
        (535, 325),
        (380, 110),
        (273, 525),
        (910, 278),
        (418, 99),
        (494, 654),
        (518, 239),
        (638, 335),
        (77, 894),
    ],
    salt: "c55fb6dffc7c9e08eb2d702f",
};
// Sealed with `cargo xtask seal --name KEY_CHECK_PATH`. Plaintext is not stored anywhere.
pub const KEY_CHECK_PATH: Sealed = Sealed {
    ciphertext: &[
        0xA1, 0xE2, 0x30, 0x9D, 0x34, 0x6E, 0x94, 0x33, 0x38, 0x6D, 0x0E, 0x16, 0x98, 0xB3, 0xD8,
        0x09, 0xA7, 0x72, 0x1A, 0x6C, 0x6F, 0x7E, 0x64, 0xBE, 0x71, 0xFE, 0x5E, 0x3A, 0xFE, 0x5E,
        0x7F, 0xAF, 0xDB, 0x4F, 0x66, 0x18, 0xCC, 0x52, 0xFE, 0x74, 0x10, 0x49, 0x14, 0x6A, 0x83,
        0xAB, 0x67, 0xA5, 0x2E,
    ],
    nonce_b64: "xUYS8SfIQM7lHyg0",
    table: &[
        (505, 553),
        (210, 253),
        (968, 808),
        (441, 941),
        (252, 167),
        (333, 261),
        (254, 59),
        (65, 219),
        (825, 465),
        (361, 436),
        (436, 293),
        (138, 34),
        (217, 49),
        (74, 233),
        (289, 245),
        (211, 464),
    ],
    salt: "b47a787f30a1a0b527b9cf67",
};
// Sealed with `cargo xtask seal --name KEY_USER_SUFFIX`. Plaintext is not stored anywhere.
pub const KEY_USER_SUFFIX: Sealed = Sealed {
    ciphertext: &[
        0x01, 0x63, 0xD6, 0xBB, 0x9C, 0x32, 0x7B, 0x95, 0x15, 0xA0, 0x03, 0x9E, 0xD4, 0xAD, 0x95,
        0x51, 0xAD, 0xEF, 0x01, 0xE8, 0xB5, 0x8D, 0x69, 0x95, 0x5E,
    ],
    nonce_b64: "FKaJQdZuoWdlntYa",
    table: &[
        (11, 957),
        (300, 820),
        (530, 337),
        (831, 809),
        (447, 481),
        (722, 208),
        (737, 617),
        (970, 201),
        (609, 478),
        (150, 362),
        (823, 97),
        (164, 601),
        (788, 712),
        (145, 554),
        (742, 786),
        (792, 28),
    ],
    salt: "5769c95c0a63ba7de11dc5e9",
};
// Sealed with `cargo xtask seal --name KEY_VALIDATE_SUFFIX`. Plaintext is not stored anywhere.
pub const KEY_VALIDATE_SUFFIX: Sealed = Sealed {
    ciphertext: &[
        0x4F, 0x97, 0x33, 0xAC, 0xAE, 0x5D, 0x87, 0x75, 0x4F, 0x51, 0x24, 0x28, 0xA4, 0xFE, 0x45,
        0x89, 0xC4, 0x89, 0xCB, 0xB8, 0xE9, 0xC0, 0x50, 0x84, 0x03, 0x0C,
    ],
    nonce_b64: "Gl22j2TGLM2Rkncg",
    table: &[
        (436, 859),
        (128, 163),
        (475, 169),
        (395, 152),
        (188, 385),
        (841, 633),
        (949, 687),
        (809, 937),
        (259, 955),
        (785, 855),
        (52, 745),
        (737, 63),
        (381, 347),
        (934, 912),
        (938, 656),
        (585, 762),
    ],
    salt: "d347de666ef18bcb074ede87",
};
