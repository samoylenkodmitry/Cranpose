#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
use super::have_avx2_fma;
use crate::range_coder::RangeCoder;

pub const CELT_PVQ_U_DATA: [u32; 1272] = [
    1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 3, 5, 7, 9, 11, 13, 15, 17, 19, 21, 23, 25, 27, 29, 31, 33, 35, 37, 39, 41, 43, 45, 47, 49,
    51, 53, 55, 57, 59, 61, 63, 65, 67, 69, 71, 73, 75, 77, 79, 81, 83, 85, 87, 89, 91, 93, 95, 97,
    99, 101, 103, 105, 107, 109, 111, 113, 115, 117, 119, 121, 123, 125, 127, 129, 131, 133, 135,
    137, 139, 141, 143, 145, 147, 149, 151, 153, 155, 157, 159, 161, 163, 165, 167, 169, 171, 173,
    175, 177, 179, 181, 183, 185, 187, 189, 191, 193, 195, 197, 199, 201, 203, 205, 207, 209, 211,
    213, 215, 217, 219, 221, 223, 225, 227, 229, 231, 233, 235, 237, 239, 241, 243, 245, 247, 249,
    251, 253, 255, 257, 259, 261, 263, 265, 267, 269, 271, 273, 275, 277, 279, 281, 283, 285, 287,
    289, 291, 293, 295, 297, 299, 301, 303, 305, 307, 309, 311, 313, 315, 317, 319, 321, 323, 325,
    327, 329, 331, 333, 335, 337, 339, 341, 343, 345, 347, 349, 351, 13, 25, 41, 61, 85, 113, 145,
    181, 221, 265, 313, 365, 421, 481, 545, 613, 685, 761, 841, 925, 1013, 1105, 1201, 1301, 1405,
    1513, 1625, 1741, 1861, 1985, 2113, 2245, 2381, 2521, 2665, 2813, 2965, 3121, 3281, 3445, 3613,
    3785, 3961, 4141, 4325, 4513, 4705, 4901, 5101, 5305, 5513, 5725, 5941, 6161, 6385, 6613, 6845,
    7081, 7321, 7565, 7813, 8065, 8321, 8581, 8845, 9113, 9385, 9661, 9941, 10225, 10513, 10805,
    11101, 11401, 11705, 12013, 12325, 12641, 12961, 13285, 13613, 13945, 14281, 14621, 14965,
    15313, 15665, 16021, 16381, 16745, 17113, 17485, 17861, 18241, 18625, 19013, 19405, 19801,
    20201, 20605, 21013, 21425, 21841, 22261, 22685, 23113, 23545, 23981, 24421, 24865, 25313,
    25765, 26221, 26681, 27145, 27613, 28085, 28561, 29041, 29525, 30013, 30505, 31001, 31501,
    32005, 32513, 33025, 33541, 34061, 34585, 35113, 35645, 36181, 36721, 37265, 37813, 38365,
    38921, 39481, 40045, 40613, 41185, 41761, 42341, 42925, 43513, 44105, 44701, 45301, 45905,
    46513, 47125, 47741, 48361, 48985, 49613, 50245, 50881, 51521, 52165, 52813, 53465, 54121,
    54781, 55445, 56113, 56785, 57461, 58141, 58825, 59513, 60205, 60901, 61601, 63, 129, 231, 377,
    575, 833, 1159, 1561, 2047, 2625, 3303, 4089, 4991, 6017, 7175, 8473, 9919, 11521, 13287,
    15225, 17343, 19649, 22151, 24857, 27775, 30913, 34279, 37881, 41727, 45825, 50183, 54809,
    59711, 64897, 70375, 76153, 82239, 88641, 95367, 102425, 109823, 117569, 125671, 134137,
    142975, 152193, 161799, 171801, 182207, 193025, 204263, 215929, 228031, 240577, 253575, 267033,
    280959, 295361, 310247, 325625, 341503, 357889, 374791, 392217, 410175, 428673, 447719, 467321,
    487487, 508225, 529543, 551449, 573951, 597057, 620775, 645113, 670079, 695681, 721927, 748825,
    776383, 804609, 833511, 863097, 893375, 924353, 956039, 988441, 1021567, 1055425, 1090023,
    1125369, 1161471, 1198337, 1235975, 1274393, 1313599, 1353601, 1394407, 1436025, 1478463,
    1521729, 1565831, 1610777, 1656575, 1703233, 1750759, 1799161, 1848447, 1898625, 1949703,
    2001689, 2054591, 2108417, 2163175, 2218873, 2275519, 2333121, 2391687, 2451225, 2511743,
    2573249, 2635751, 2699257, 2763775, 2829313, 2895879, 2963481, 3032127, 3101825, 3172583,
    3244409, 3317311, 3391297, 3466375, 3542553, 3619839, 3698241, 3777767, 3858425, 3940223,
    4023169, 4107271, 4192537, 4278975, 4366593, 4455399, 4545401, 4636607, 4729025, 4822663,
    4917529, 5013631, 5110977, 5209575, 5309433, 5410559, 5512961, 5616647, 5721625, 5827903,
    5935489, 6044391, 6154617, 6266175, 6379073, 6493319, 6608921, 6725887, 6844225, 6963943,
    7085049, 7207551, 321, 681, 1289, 2241, 3649, 5641, 8361, 11969, 16641, 22569, 29961, 39041,
    50049, 63241, 78889, 97281, 118721, 143529, 172041, 204609, 241601, 283401, 330409, 383041,
    441729, 506921, 579081, 658689, 746241, 842249, 947241, 1061761, 1186369, 1321641, 1468169,
    1626561, 1797441, 1981449, 2179241, 2391489, 2618881, 2862121, 3121929, 3399041, 3694209,
    4008201, 4341801, 4695809, 5071041, 5468329, 5888521, 6332481, 6801089, 7295241, 7815849,
    8363841, 8940161, 9545769, 10181641, 10848769, 11548161, 12280841, 13047849, 13850241,
    14689089, 15565481, 16480521, 17435329, 18431041, 19468809, 20549801, 21675201, 22846209,
    24064041, 25329929, 26645121, 28010881, 29428489, 30899241, 32424449, 34005441, 35643561,
    37340169, 39096641, 40914369, 42794761, 44739241, 46749249, 48826241, 50971689, 53187081,
    55473921, 57833729, 60268041, 62778409, 65366401, 68033601, 70781609, 73612041, 76526529,
    79526721, 82614281, 85790889, 89058241, 92418049, 95872041, 99421961, 103069569, 106816641,
    110664969, 114616361, 118672641, 122835649, 127107241, 131489289, 135983681, 140592321,
    145317129, 150160041, 155123009, 160208001, 165417001, 170752009, 176215041, 181808129,
    187533321, 193392681, 199388289, 205522241, 211796649, 218213641, 224775361, 231483969,
    238341641, 245350569, 252512961, 259831041, 267307049, 274943241, 282741889, 290705281,
    298835721, 307135529, 315607041, 324252609, 333074601, 342075401, 351257409, 360623041,
    370174729, 379914921, 389846081, 399970689, 410291241, 420810249, 431530241, 442453761,
    453583369, 464921641, 476471169, 488234561, 500214441, 512413449, 524834241, 537479489,
    550351881, 563454121, 576788929, 590359041, 604167209, 618216201, 632508801, 1683, 3653, 7183,
    13073, 22363, 36365, 56695, 85305, 124515, 177045, 246047, 335137, 448427, 590557, 766727,
    982729, 1244979, 1560549, 1937199, 2383409, 2908411, 3522221, 4235671, 5060441, 6009091,
    7095093, 8332863, 9737793, 11326283, 13115773, 15124775, 17372905, 19880915, 22670725,
    25765455, 29189457, 32968347, 37129037, 41699767, 46710137, 52191139, 58175189, 64696159,
    71789409, 79491819, 87841821, 96879431, 106646281, 117185651, 128542501, 140763503, 153897073,
    167993403, 183104493, 199284183, 216588185, 235074115, 254801525, 275831935, 298228865,
    322057867, 347386557, 374284647, 402823977, 433078547, 465124549, 499040399, 534906769,
    572806619, 612825229, 655050231, 699571641, 746481891, 795875861, 847850911, 902506913,
    959946283, 1020274013, 1083597703, 1150027593, 1219676595, 1292660325, 1369097135, 1449108145,
    1532817275, 1620351277, 1711839767, 1807415257, 1907213187, 2011371957, 2120032959, 8989,
    19825, 40081, 75517, 134245, 227305, 369305, 579125, 880685, 1303777, 1884961, 2668525,
    3707509, 5064793, 6814249, 9041957, 11847485, 15345233, 19665841, 24957661, 31388293, 39146185,
    48442297, 59511829, 72616013, 88043969, 106114625, 127178701, 151620757, 179861305, 212358985,
    249612805, 292164445, 340600625, 395555537, 457713341, 527810725, 606639529, 695049433,
    793950709, 904317037, 1027188385, 1163673953, 1314955181, 1482288821, 1667010073, 1870535785,
    2094367717, 48639, 108545, 224143, 433905, 795455, 1392065, 2340495, 3800305, 5984767, 9173505,
    13726991, 20103025, 28875327, 40754369, 56610575, 77500017, 104692735, 139703809, 184327311,
    240673265, 311207743, 398796225, 506750351, 638878193, 799538175, 993696769, 1226990095,
    1505789553, 1837271615, 2229491905, 265729, 598417, 1256465, 2485825, 4673345, 8405905,
    14546705, 24331777, 39490049, 62390545, 96220561, 145198913, 214828609, 312193553, 446304145,
    628496897, 872893441, 1196924561, 1621925137, 2173806145, 1462563, 3317445, 7059735, 14218905,
    27298155, 50250765, 89129247, 152951073, 254831667, 413442773, 654862247, 1014889769,
    1541911931, 2300409629, 3375210671, 8097453, 18474633, 39753273, 81270333, 158819253,
    298199265, 540279585, 948062325, 1616336765, 45046719, 103274625, 224298231, 464387817,
    921406335, 1759885185, 3248227095, 251595969, 579168825, 1267854873, 2653649025, 1409933619,
];

const CELT_PVQ_U_ROW: [u32; 15] = [
    0, 176, 351, 525, 698, 870, 1041, 1131, 1178, 1207, 1226, 1240, 1248, 1254, 1257,
];

#[inline(always)]
pub fn celt_pvq_u_lookup(n: u32, k: u32) -> u32 {
    let r = n.min(k) as usize;
    let c = n.max(k) as usize;

    if r >= CELT_PVQ_U_ROW.len() {
        return compute_u(n, k);
    }
    // Both indices are bounds-checked on the lines above, so the compiler
    // folds the indexing checks away and the table reads need no `unsafe`.
    let row_base = CELT_PVQ_U_ROW[r];
    let idx = row_base as usize + c;
    if idx >= CELT_PVQ_U_DATA.len() {
        return compute_u(n, k);
    }
    CELT_PVQ_U_DATA[idx]
}

const MAX_PVQ_K: usize = 128;
const MAX_PVQ_U: usize = MAX_PVQ_K + 2;
pub const MAX_PVQ_N: usize = 352;

fn compute_u(n: u32, k: u32) -> u32 {
    if n == 0 {
        return if k == 0 { 1 } else { 0 };
    }
    if n == 1 {
        return 1;
    }
    let mut u = [0u32; MAX_PVQ_U];
    u[0] = 0;
    u[1] = 1;
    for ki in 2..=(k + 1) as usize {
        u[ki] = (ki as u32 * 2).wrapping_sub(1);
    }
    let mut curr_n = n;
    while curr_n > 2 {
        unext(&mut u[1..], (k + 1) as usize, 1);
        curr_n -= 1;
    }
    u[k as usize]
}

#[inline(always)]
pub fn celt_pvq_v(n: u32, k: u32) -> u32 {
    celt_pvq_u_lookup(n, k).wrapping_add(celt_pvq_u_lookup(n, k + 1))
}

fn unext(u: &mut [u32], len: usize, mut u0: u32) {
    let mut j = 1;
    while j < len {
        let u1 = u[j].wrapping_add(u[j - 1]).wrapping_add(u0);
        u[j - 1] = u0;
        u0 = u1;
        j += 1;
    }
    u[j - 1] = u0;
}

#[inline(always)]
pub fn icwrs(n: u32, _k: u32, y: &[i32]) -> u32 {
    if n == 1 {
        return if y[0] < 0 { 1 } else { 0 };
    }
    debug_assert!(n >= 2, "icwrs: n must be >= 2");
    let mut j = (n - 1) as usize;

    let mut i: u32 = if y[j] < 0 { 1 } else { 0 };
    let mut k = y[j].unsigned_abs();

    while j > 0 {
        j -= 1;
        let yj = y[j];
        let m = n - j as u32;
        i = i.wrapping_add(celt_pvq_u_lookup(m, k));
        k += yj.unsigned_abs();

        let sign_mask = yj >> 31;
        let lookup = (sign_mask as u32) & celt_pvq_u_lookup(m, k + 1);
        i = i.wrapping_add(lookup);
    }
    i
}

#[inline(always)]
pub fn cwrsi(n: u32, k: u32, mut i: u32, y: &mut [i32]) {
    debug_assert!(k > 0, "cwrsi: k must be > 0");

    if n == 1 {
        let s = -(i as i32);
        y[0] = ((k as i32) + s) ^ s;
        return;
    }

    let mut curr_n = n;

    let mut curr_k = k as i32;
    let mut j = 0usize;

    while curr_n > 2 {
        if curr_k >= curr_n as i32 {
            let p_kp1 = celt_pvq_u_lookup(curr_n, (curr_k + 1) as u32);
            let s: i32 = if i >= p_kp1 {
                i -= p_kp1;
                -1
            } else {
                0
            };
            let k0 = curr_k;
            let q = celt_pvq_u_lookup(curr_n, curr_n);
            let mut p;
            if q > i {
                curr_k = curr_n as i32;
                loop {
                    curr_k -= 1;
                    p = celt_pvq_u_lookup(curr_n, curr_k.max(0) as u32);
                    if p <= i || curr_k <= 0 {
                        break;
                    }
                }
            } else {
                p = celt_pvq_u_lookup(curr_n, curr_k as u32);
                while p > i && curr_k > 0 {
                    curr_k -= 1;
                    p = celt_pvq_u_lookup(curr_n, curr_k as u32);
                }
            }
            i -= p;
            let val = k0 - curr_k;
            y[j] = (val + s) ^ s;
        } else {
            let p_k = celt_pvq_u_lookup(curr_k as u32, curr_n);
            let p_kp1 = celt_pvq_u_lookup((curr_k + 1) as u32, curr_n);
            if p_k <= i && i < p_kp1 {
                i -= p_k;
                y[j] = 0;
                j += 1;
                curr_n -= 1;
                continue;
            }
            let s: i32 = if i >= p_kp1 {
                i -= p_kp1;
                -1
            } else {
                0
            };
            let k0 = curr_k;

            let mut p;
            loop {
                curr_k -= 1;
                p = celt_pvq_u_lookup(curr_k.max(0) as u32, curr_n);
                if p <= i || curr_k <= 0 {
                    break;
                }
            }
            i -= p;
            let val = k0 - curr_k;
            y[j] = (val + s) ^ s;
        }
        j += 1;
        curr_n -= 1;
    }

    let p2 = (2u32).wrapping_mul(curr_k as u32).wrapping_add(1);
    let s2: i32 = if i >= p2 {
        i -= p2;
        -1
    } else {
        0
    };
    let k0 = curr_k;
    curr_k = ((i + 1) >> 1) as i32;
    if curr_k > 0 {
        i -= 2 * curr_k as u32 - 1;
    }
    y[j] = ((k0 - curr_k) + s2) ^ s2;
    j += 1;

    let s1 = -(i as i32);
    y[j] = (curr_k + s1) ^ s1;
}

#[inline(always)]
pub fn encode_pulses(y: &[i32], n: u32, k: u32, rc: &mut RangeCoder) {
    if k == 0 {
        return;
    }
    let fl = icwrs(n, k, y);
    let ft = celt_pvq_v(n, k);
    debug_assert!(fl < ft, "encode_pulses: fl={fl} >= ft={ft}, n={n}, k={k}");
    rc.enc_uint(fl, ft);
}

#[inline(always)]
pub fn decode_pulses(y: &mut [i32], n: u32, k: u32, rc: &mut RangeCoder) {
    if k == 0 {
        for i in 0..n as usize {
            y[i] = 0;
        }
        return;
    }
    let ft = celt_pvq_v(n, k);
    let fl = rc.dec_uint(ft).min(ft.saturating_sub(1));
    cwrsi(n, k, fl, y);
}

#[inline(always)]
fn pvq_search_n2(x: &[f32], y: &mut [i32], k: i32) {
    debug_assert!(x.len() >= 2 && y.len() >= 2);

    let abs_x0 = x[0].abs();
    let abs_x1 = x[1].abs();
    let sum = abs_x0 + abs_x1;

    if sum < 1e-15 {
        y[0] = k;
        y[1] = 0;
        return;
    }

    let rcp_sum = 1.0 / sum;
    let y0 = (k as f32 * abs_x0 * rcp_sum + 0.5).floor() as i32;
    let y0 = y0.clamp(0, k);
    let y1 = k - y0;

    y[0] = if x[0] >= 0.0 { y0 } else { -y0 };
    y[1] = if x[1] >= 0.0 { y1 } else { -y1 };
}

#[inline]
fn pvq_search_n4(x: &[f32], y: &mut [i32], k: i32) {
    debug_assert!(x.len() >= 4 && y.len() >= 4);

    if k == 0 {
        y[0] = 0;
        y[1] = 0;
        y[2] = 0;
        y[3] = 0;
        return;
    }

    #[cfg(target_arch = "x86_64")]
    unsafe {
        use std::arch::x86_64::*;

        let sign_mask = _mm_castsi128_ps(_mm_set1_epi32(0x7FFF_FFFFu32 as i32));
        let vx = _mm_loadu_ps(x.as_ptr());
        let vabs = _mm_and_ps(vx, sign_mask);

        let vzero_f = _mm_setzero_ps();

        let vneg_mask = _mm_cmplt_ps(vx, vzero_f);

        let vsigns = _mm_and_si128(_mm_castps_si128(vneg_mask), _mm_set1_epi32(1));

        let vabs_x = vabs;
        let mut vy2f = _mm_setzero_ps();
        let mut vy = _mm_setzero_si128();
        let mut xy = 0.0f32;
        let mut yy = 0.0f32;

        let vtwo = _mm_set1_ps(2.0);

        let vone_i = _mm_set1_epi32(1);

        for _ in 0..k {
            let vxy = _mm_set1_ps(xy);
            let vrxy = _mm_add_ps(vabs_x, vxy);
            let vyy1 = _mm_add_ps(vy2f, _mm_set1_ps(yy + 1.0));

            let vscore = _mm_mul_ps(vrxy, _mm_rsqrt_ps(vyy1));

            let s0 = _mm_cvtss_f32(vscore);
            let s1 = _mm_cvtss_f32(_mm_shuffle_ps(vscore, vscore, 0b01_01_01_01));
            let s2 = _mm_cvtss_f32(_mm_shuffle_ps(vscore, vscore, 0b10_10_10_10));
            let s3 = _mm_cvtss_f32(_mm_shuffle_ps(vscore, vscore, 0b11_11_11_11));
            let mut best_score = s0;
            let mut best_i: u32 = 0;
            if s1 > best_score {
                best_score = s1;
                best_i = 1;
            }
            if s2 > best_score {
                best_score = s2;
                best_i = 2;
            }
            if s3 > best_score {
                best_i = 3;
            }
            let _ = best_score;

            let vbest = _mm_set1_epi32(best_i as i32);
            let vlane = _mm_setr_epi32(0, 1, 2, 3);
            let vmask = _mm_castsi128_ps(_mm_cmpeq_epi32(vlane, vbest));

            let vpick_ax = _mm_and_ps(vabs_x, vmask);

            let vpick_ax_hi = _mm_movehl_ps(vpick_ax, vpick_ax);
            let vpick_ax2 = _mm_add_ps(vpick_ax, vpick_ax_hi);
            let vpick_ax3 = _mm_add_ss(vpick_ax2, _mm_shuffle_ps(vpick_ax2, vpick_ax2, 1));
            xy += _mm_cvtss_f32(vpick_ax3);

            let vpick_ryy = _mm_and_ps(vyy1, vmask);
            let vpick_ryy_hi = _mm_movehl_ps(vpick_ryy, vpick_ryy);
            let vpick_ryy2 = _mm_add_ps(vpick_ryy, vpick_ryy_hi);
            let vpick_ryy3 = _mm_add_ss(vpick_ryy2, _mm_shuffle_ps(vpick_ryy2, vpick_ryy2, 1));
            yy = _mm_cvtss_f32(vpick_ryy3);

            let vadd2 = _mm_and_ps(vtwo, vmask);
            vy2f = _mm_add_ps(vy2f, vadd2);

            let vadd1 = _mm_and_si128(vone_i, _mm_castps_si128(vmask));
            vy = _mm_add_epi32(vy, vadd1);
        }

        let vneg_s = _mm_sub_epi32(_mm_setzero_si128(), vsigns);
        let vy_xor = _mm_xor_si128(vy, vneg_s);
        let vy_out = _mm_add_epi32(vy_xor, vsigns);
        _mm_storeu_si128(y.as_mut_ptr() as *mut __m128i, vy_out);
    }

    #[cfg(not(target_arch = "x86_64"))]
    {
        let ax0 = x[0].abs();
        let ax1 = x[1].abs();
        let ax2 = x[2].abs();
        let ax3 = x[3].abs();
        let s0 = (x[0] < 0.0) as i32;
        let s1 = (x[1] < 0.0) as i32;
        let s2 = (x[2] < 0.0) as i32;
        let s3 = (x[3] < 0.0) as i32;
        let mut xy = 0.0f32;
        let mut yy = 0.0f32;
        let mut y2f0 = 0.0f32;
        let mut y2f1 = 0.0f32;
        let mut y2f2 = 0.0f32;
        let mut y2f3 = 0.0f32;
        let mut y0 = 0i32;
        let mut y1 = 0i32;
        let mut y2 = 0i32;
        let mut y3 = 0i32;
        for _ in 0..k {
            let rxy0 = xy + ax0;
            let sq0 = rxy0 * rxy0;
            let ryy0 = yy + y2f0 + 1.0;
            let rxy1 = xy + ax1;
            let sq1 = rxy1 * rxy1;
            let ryy1 = yy + y2f1 + 1.0;
            let rxy2 = xy + ax2;
            let sq2 = rxy2 * rxy2;
            let ryy2 = yy + y2f2 + 1.0;
            let rxy3 = xy + ax3;
            let sq3 = rxy3 * rxy3;
            let ryy3 = yy + y2f3 + 1.0;
            let mut bsq = sq0;
            let mut bden = ryy0;
            let mut best_i: u32 = 0;
            if bden * sq1 > ryy1 * bsq {
                bsq = sq1;
                bden = ryy1;
                best_i = 1;
            }
            if bden * sq2 > ryy2 * bsq {
                bsq = sq2;
                bden = ryy2;
                best_i = 2;
            }
            if bden * sq3 > ryy3 * bsq {
                best_i = 3;
            }
            let _ = bsq;
            match best_i {
                0 => {
                    xy += ax0;
                    yy = ryy0;
                    y2f0 += 2.0;
                    y0 += 1;
                }
                1 => {
                    xy += ax1;
                    yy = ryy1;
                    y2f1 += 2.0;
                    y1 += 1;
                }
                2 => {
                    xy += ax2;
                    yy = ryy2;
                    y2f2 += 2.0;
                    y2 += 1;
                }
                _ => {
                    xy += ax3;
                    yy = ryy3;
                    y2f3 += 2.0;
                    y3 += 1;
                }
            }
        }
        y[0] = (y0 ^ -s0) + s0;
        y[1] = (y1 ^ -s1) + s1;
        y[2] = (y2 ^ -s2) + s2;
        y[3] = (y3 ^ -s3) + s3;
    }
}

#[inline(always)]
pub fn pvq_search(x: &[f32], y: &mut [i32], k: i32, n: usize) {
    if k == 1 {
        let mut best_i = 0;
        let mut best_abs = x[0].abs();
        for i in 1..n {
            let abs_xi = x[i].abs();
            if abs_xi > best_abs {
                best_abs = abs_xi;
                best_i = i;
            }
        }
        for j in 0..n {
            y[j] = 0;
        }
        let sign: i32 = if x[best_i] >= 0.0 { 1 } else { -1 };
        y[best_i] = sign;
        return;
    }

    if n == 2 {
        pvq_search_n2(x, y, k);
        return;
    }

    if n == 4 {
        pvq_search_n4(x, y, k);
        return;
    }

    if n >= 32 {
        pvq_search_fast_select(x, y, k, n);
        return;
    }

    // Everything below 32 goes through the scalar search, which reproduces
    // libopus `op_pvq_search_c` exactly. A NEON kernel used to take n <= 16
    // here: its k <= 4 branch compared `Rxy/Ryy` where the reference maximises
    // `Rxy^2/Ryy`, and its k > 4 branch scored with `vrsqrteq_f32` — an
    // eight-bit estimate — and broke ties by float equality against a running
    // maximum. Measured against a transcription of the reference it chose a
    // different codeword in 470 of 1880 cases and a worse-scoring one in 450,
    // giving up as much as 29.7% of the search objective. libopus does not
    // vectorise this search on ARM either. See `simd_tests`.

    #[cfg(target_arch = "x86_64")]
    if k > 4 && have_avx2_fma() {
        unsafe {
            pvq_search_avx2(x, y, k, n);
        }
        return;
    }

    pvq_search_scalar(x, y, k, n);
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn pvq_fast_select_init_neon(
    x: &[f32],
    n: usize,
    abs_x: &mut [f32; MAX_PVQ_N],
    signs: &mut [i32; MAX_PVQ_N],
) -> f32 {
    use std::arch::aarch64::*;

    // The kernel reads `n` inputs and writes `n` entries of each output; the
    // trims below are what put those stores inside the fixed-size buffers and
    // the loads inside `x`.
    let x = &x[..n];
    let abs_x = &mut abs_x[..n];
    let signs = &mut signs[..n];

    let mut sum_vec = vdupq_n_f32(0.0);
    let mut i = 0;

    while i + 16 <= n {
        let vx0 = vld1q_f32(x.as_ptr().add(i));
        let vx1 = vld1q_f32(x.as_ptr().add(i + 4));
        let vx2 = vld1q_f32(x.as_ptr().add(i + 8));
        let vx3 = vld1q_f32(x.as_ptr().add(i + 12));

        let vabs0 = vabsq_f32(vx0);
        let vabs1 = vabsq_f32(vx1);
        let vabs2 = vabsq_f32(vx2);
        let vabs3 = vabsq_f32(vx3);

        vst1q_f32(abs_x.as_mut_ptr().add(i), vabs0);
        vst1q_f32(abs_x.as_mut_ptr().add(i + 4), vabs1);
        vst1q_f32(abs_x.as_mut_ptr().add(i + 8), vabs2);
        vst1q_f32(abs_x.as_mut_ptr().add(i + 12), vabs3);

        sum_vec = vaddq_f32(sum_vec, vabs0);
        sum_vec = vaddq_f32(sum_vec, vabs1);
        sum_vec = vaddq_f32(sum_vec, vabs2);
        sum_vec = vaddq_f32(sum_vec, vabs3);

        for j in 0..16 {
            signs[i + j] = if x[i + j] < 0.0 { -1i32 } else { 1i32 };
        }

        i += 16;
    }

    while i + 8 <= n {
        let vx0 = vld1q_f32(x.as_ptr().add(i));
        let vx1 = vld1q_f32(x.as_ptr().add(i + 4));

        let vabs0 = vabsq_f32(vx0);
        let vabs1 = vabsq_f32(vx1);

        vst1q_f32(abs_x.as_mut_ptr().add(i), vabs0);
        vst1q_f32(abs_x.as_mut_ptr().add(i + 4), vabs1);

        sum_vec = vaddq_f32(sum_vec, vabs0);
        sum_vec = vaddq_f32(sum_vec, vabs1);

        for j in 0..8 {
            signs[i + j] = if x[i + j] < 0.0 { -1i32 } else { 1i32 };
        }

        i += 8;
    }

    while i + 4 <= n {
        let vx = vld1q_f32(x.as_ptr().add(i));
        let vabs = vabsq_f32(vx);
        vst1q_f32(abs_x.as_mut_ptr().add(i), vabs);
        sum_vec = vaddq_f32(sum_vec, vabs);

        for j in 0..4 {
            signs[i + j] = if x[i + j] < 0.0 { -1i32 } else { 1i32 };
        }

        i += 4;
    }

    let mut sum = vaddvq_f32(sum_vec);

    for j in i..n {
        let abs_xi = x[j].abs();
        abs_x[j] = abs_xi;
        sum += abs_xi;
        signs[j] = if x[j] < 0.0 { -1i32 } else { 1i32 };
    }

    sum
}

#[inline]
pub fn pvq_search_fast_select(x: &[f32], y: &mut [i32], k: i32, n: usize) -> f32 {
    let mut k = k;
    let mut yy = 0.0f32;
    let mut xy = 0.0f32;

    y[..n].fill(0);

    if k <= 0 {
        return 0.0;
    }

    let mut abs_x_mu = [0.0f32; MAX_PVQ_N];
    let mut signs_mu = [0i32; MAX_PVQ_N];

    #[cfg(target_arch = "aarch64")]
    let sum = unsafe { pvq_fast_select_init_neon(x, n, &mut abs_x_mu, &mut signs_mu) };
    #[cfg(not(target_arch = "aarch64"))]
    let sum = {
        let mut s = 0.0f32;
        for i in 0..n {
            abs_x_mu[i] = x[i].abs();
            signs_mu[i] = if x[i] < 0.0 { -1i32 } else { 1i32 };
            s += abs_x_mu[i];
        }
        s
    };

    // Everything below indexes the first `n` entries; trim once so the
    // compiler carries the bound instead of a raw pointer asserting it.
    let abs_x = &abs_x_mu[..n];
    let signs = &signs_mu[..n];
    let y = &mut y[..n];

    if k > (n >> 1) as i32 && sum > 1e-15 {
        let rcp = (k as f32 + 0.8) / sum;

        for i in 0..n {
            let yi = (abs_x[i] * rcp) as i32;
            y[i] = yi;
            let yf = yi as f32;
            yy += yf * yf;
            xy += yf * abs_x[i];
            k -= yi;
        }

        if k > n as i32 + 3 {
            let tmp = k as f32;
            yy += tmp * tmp + tmp * y[0] as f32;
            y[0] += k;
            k = 0;
        }
    }

    const BATCH_SIZE: i32 = 4;

    if k < BATCH_SIZE * 2 || n < 16 {
        #[cfg(target_arch = "aarch64")]
        {
            use std::arch::aarch64::*;
            let mut y2f_mu = [0.0f32; MAX_PVQ_N];
            for i in 0..n {
                y2f_mu[i] = 2.0 * y[i] as f32;
            }
            let y2f = &mut y2f_mu[..n];

            let n4 = n & !3;
            while k > 0 {
                yy += 1.0;
                let mut best_id: usize = 0;
                // SAFETY: the vector loop reads four lanes at a time and stops
                // at `n4`, the multiple of four at or below `n`, so every load
                // stays inside the trimmed slices.
                let mut vmax = unsafe {
                    let vxy = vdupq_n_f32(xy);
                    let vyy = vdupq_n_f32(yy);
                    let mut vmax = vdupq_n_f32(0.0);
                    let mut i = 0;
                    while i < n4 {
                        let vx = vld1q_f32(abs_x.as_ptr().add(i));
                        let vy = vld1q_f32(y2f.as_ptr().add(i));
                        let rxy = vaddq_f32(vx, vxy);
                        let ryy = vaddq_f32(vy, vyy);
                        let inv_sqrt = vrsqrteq_f32(ryy);
                        let score = vmulq_f32(rxy, inv_sqrt);
                        vmax = vmaxq_f32(vmax, score);
                        let mx = vmaxvq_f32(vmax);
                        // Lanes are read out of the register directly; a slice
                        // over `&score` would spill it to the stack first.
                        if vgetq_lane_f32::<0>(score) == mx {
                            best_id = i;
                        }
                        if vgetq_lane_f32::<1>(score) == mx {
                            best_id = i + 1;
                        }
                        if vgetq_lane_f32::<2>(score) == mx {
                            best_id = i + 2;
                        }
                        if vgetq_lane_f32::<3>(score) == mx {
                            best_id = i + 3;
                        }
                        i += 4;
                    }
                    vmax
                };

                for i in n4..n {
                    let rxy = xy + abs_x[i];
                    let ryy = yy + y2f[i];
                    let score = rxy * (1.0 / ryy.sqrt());
                    // SAFETY: register-only NEON, no memory touched.
                    let current_max = unsafe { vmaxvq_f32(vmax) };
                    if score > current_max {
                        best_id = i;
                        vmax = unsafe { vsetq_lane_f32(score, vmax, 0) };
                    }
                }

                xy += abs_x[best_id];
                yy += y2f[best_id];
                y2f[best_id] += 2.0;
                y[best_id] += 1;
                k -= 1;
            }
        }
        #[cfg(not(target_arch = "aarch64"))]
        {
            let mut y2f_mu = [0.0f32; MAX_PVQ_N];
            let y2f = &mut y2f_mu[..n];
            while k > 0 {
                yy += 1.0;
                let rxy0 = xy + abs_x[0];
                let mut best_id = 0;
                let mut best_num = rxy0 * rxy0;
                let mut best_den = yy + y2f[0];
                let mut i = 1;
                while i + 1 < n {
                    let rxy1 = xy + abs_x[i];
                    let ryy1 = yy + y2f[i];
                    let rxy1_sq = rxy1 * rxy1;
                    if best_den * rxy1_sq > ryy1 * best_num {
                        best_id = i;
                        best_num = rxy1_sq;
                        best_den = ryy1;
                    }
                    let rxy2 = xy + abs_x[i + 1];
                    let ryy2 = yy + y2f[i + 1];
                    let rxy2_sq = rxy2 * rxy2;
                    if best_den * rxy2_sq > ryy2 * best_num {
                        best_id = i + 1;
                        best_num = rxy2_sq;
                        best_den = ryy2;
                    }
                    i += 2;
                }
                if i < n {
                    let rxy = xy + abs_x[i];
                    let ryy = yy + y2f[i];
                    let rxy_sq = rxy * rxy;
                    if best_den * rxy_sq > ryy * best_num {
                        best_id = i;
                    }
                }
                xy += abs_x[best_id];
                yy += y2f[best_id];
                y2f[best_id] += 2.0;
                y[best_id] += 1;
                k -= 1;
            }
        }
    } else {
        let mut y2f_mu = [0.0f32; MAX_PVQ_N];
        for i in 0..n {
            y2f_mu[i] = 2.0 * y[i] as f32;
        }
        let y2f = &mut y2f_mu[..n];
        let mut scores_mu = [(0.0f32, 0usize); MAX_PVQ_N];

        while k > 0 {
            let batch = BATCH_SIZE.min(k);

            let scores = &mut scores_mu[..n];
            for i in 0..n {
                let rxy = xy + abs_x[i];
                let ryy = yy + y2f[i] + 1.0;
                scores[i] = (rxy * rxy / ryy, i);
            }

            let pos = batch as usize;

            scores.select_nth_unstable_by(pos, |a, b| {
                if a.0 > b.0 {
                    std::cmp::Ordering::Less
                } else if a.0 < b.0 {
                    std::cmp::Ordering::Greater
                } else {
                    std::cmp::Ordering::Equal
                }
            });

            for b in 0..batch as usize {
                let idx = scores[b].1;
                xy += abs_x[idx];
                yy += y2f[idx] + 1.0;
                y2f[idx] += 2.0;
                y[idx] += 1;
            }

            k -= batch;
        }
    }

    for i in 0..n {
        y[i] *= signs[i];
    }

    yy
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn pvq_search_scalar_init_neon(
    x: &[f32],
    n: usize,
    abs_x: &mut [f32; 32],
    sign_x: &mut [i32; 32],
) -> f32 {
    use std::arch::aarch64::*;

    // The kernel reads `n` inputs and writes `n` entries of each output; the
    // trims below are what put those stores inside the fixed-size buffers and
    // the loads inside `x`.
    let x = &x[..n];
    let abs_x = &mut abs_x[..n];
    let sign_x = &mut sign_x[..n];

    let mut sum_vec = vdupq_n_f32(0.0);
    let mut i = 0;

    while i + 16 <= n {
        let vx0 = vld1q_f32(x.as_ptr().add(i));
        let vx1 = vld1q_f32(x.as_ptr().add(i + 4));
        let vx2 = vld1q_f32(x.as_ptr().add(i + 8));
        let vx3 = vld1q_f32(x.as_ptr().add(i + 12));

        let vabs0 = vabsq_f32(vx0);
        let vabs1 = vabsq_f32(vx1);
        let vabs2 = vabsq_f32(vx2);
        let vabs3 = vabsq_f32(vx3);

        vst1q_f32(abs_x.as_mut_ptr().add(i), vabs0);
        vst1q_f32(abs_x.as_mut_ptr().add(i + 4), vabs1);
        vst1q_f32(abs_x.as_mut_ptr().add(i + 8), vabs2);
        vst1q_f32(abs_x.as_mut_ptr().add(i + 12), vabs3);

        sum_vec = vaddq_f32(sum_vec, vabs0);
        sum_vec = vaddq_f32(sum_vec, vabs1);
        sum_vec = vaddq_f32(sum_vec, vabs2);
        sum_vec = vaddq_f32(sum_vec, vabs3);

        for j in 0..16 {
            sign_x[i + j] = (x[i + j] < 0.0) as i32;
        }

        i += 16;
    }

    while i + 8 <= n {
        let vx0 = vld1q_f32(x.as_ptr().add(i));
        let vx1 = vld1q_f32(x.as_ptr().add(i + 4));

        let vabs0 = vabsq_f32(vx0);
        let vabs1 = vabsq_f32(vx1);

        vst1q_f32(abs_x.as_mut_ptr().add(i), vabs0);
        vst1q_f32(abs_x.as_mut_ptr().add(i + 4), vabs1);

        sum_vec = vaddq_f32(sum_vec, vabs0);
        sum_vec = vaddq_f32(sum_vec, vabs1);

        for j in 0..8 {
            sign_x[i + j] = (x[i + j] < 0.0) as i32;
        }

        i += 8;
    }

    while i + 4 <= n {
        let vx = vld1q_f32(x.as_ptr().add(i));
        let vabs = vabsq_f32(vx);
        vst1q_f32(abs_x.as_mut_ptr().add(i), vabs);
        sum_vec = vaddq_f32(sum_vec, vabs);

        for j in 0..4 {
            sign_x[i + j] = (x[i + j] < 0.0) as i32;
        }

        i += 4;
    }

    let mut sum = vaddvq_f32(sum_vec);

    for j in i..n {
        let xi = x[j];
        let abs_xi = xi.abs();
        abs_x[j] = abs_xi;
        sum += abs_xi;
        sign_x[j] = (xi < 0.0) as i32;
    }

    sum
}

#[inline(always)]
fn pvq_search_small_k(x: &[f32], y: &mut [i32], k: i32, n: usize) {
    debug_assert!(k <= 4 && k > 0);
    debug_assert!(n <= 31);

    let mut abs_x_buf = [0.0f32; 32];
    let mut y2f_buf = [0.0f32; 32];
    let mut sign_x_buf = [0i32; 32];

    // Trim every buffer to `n` once. From here the loops index slices the
    // compiler already knows are exactly `n` long, so the indexing carries no
    // check and needs no raw pointers to say so.
    let x = &x[..n];
    let abs_x = &mut abs_x_buf[..n];
    let y2f = &mut y2f_buf[..n];
    let sign_x = &mut sign_x_buf[..n];
    let y = &mut y[..n];

    for i in 0..n {
        let xi = x[i];
        abs_x[i] = xi.abs();
        sign_x[i] = (xi < 0.0) as i32;
    }

    let mut yy = 0.0f32;
    let mut xy = 0.0f32;

    for _ in 0..k {
        yy += 1.0;

        let rxy0 = xy + abs_x[0];
        let mut best_id = 0usize;
        let mut best_num = rxy0 * rxy0;
        let mut best_den = yy + y2f[0];

        let mut i = 1;
        while i + 1 < n {
            let rxy1 = xy + abs_x[i];
            let rxy2 = xy + abs_x[i + 1];
            let den1 = yy + y2f[i];
            let den2 = yy + y2f[i + 1];
            let rxy1_sq = rxy1 * rxy1;
            let rxy2_sq = rxy2 * rxy2;

            if best_den * rxy1_sq > den1 * best_num {
                best_id = i;
                best_num = rxy1_sq;
                best_den = den1;
            }
            if best_den * rxy2_sq > den2 * best_num {
                best_id = i + 1;
                best_num = rxy2_sq;
                best_den = den2;
            }
            i += 2;
        }
        if i < n {
            let rxy = xy + abs_x[i];
            let rxy_sq = rxy * rxy;
            let den = yy + y2f[i];
            if best_den * rxy_sq > den * best_num {
                best_id = i;
            }
        }

        xy += abs_x[best_id];
        yy += y2f[best_id];
        y2f[best_id] += 2.0;
        y[best_id] += 1;
    }

    for i in 0..n {
        let s = sign_x[i];
        y[i] = (y[i] ^ -s) + s;
    }
}
#[inline]
/// libopus `op_pvq_search_c`, for `n <= 31`.
///
/// The bound is structural: the working buffers below are fixed at 32 entries,
/// so a larger `n` writes past them. It used to be a `debug_assert!`, which
/// release builds compile out — a safe function that corrupts memory when its
/// contract is broken. `pvq_search` never breaks it; a future caller might.
fn pvq_search_scalar(x: &[f32], y: &mut [i32], k: i32, n: usize) {
    assert!(
        n <= 31,
        "pvq_search_scalar: n = {n} exceeds its 31-sample bound"
    );
    let mut k = k;
    let mut yy = 0.0f32;
    let mut xy = 0.0f32;

    y[..n].fill(0);

    if k <= 0 {
        return;
    }

    if k <= 4 {
        pvq_search_small_k(x, y, k, n);
        return;
    }

    let mut abs_x_buf = [0.0f32; 32];
    let mut y2f_buf = [0.0f32; 32];
    let mut sign_x_buf = [0i32; 32];
    let (abs_x, y2f, sign_x) = (&mut abs_x_buf, &mut y2f_buf, &mut sign_x_buf);

    #[cfg(target_arch = "aarch64")]
    let sum = unsafe { pvq_search_scalar_init_neon(x, n, abs_x, sign_x) };
    #[cfg(all(not(target_arch = "aarch64"), target_arch = "x86_64"))]
    let sum = unsafe {
        if std::arch::is_x86_feature_detected!("avx2") {
            pvq_search_scalar_init_avx2(x, n, abs_x, sign_x)
        } else {
            let mut s = 0.0f32;
            for i in 0..n {
                let xi = x[i];
                let abs_xi = xi.abs();
                abs_x[i] = abs_xi;
                s += abs_xi;
                sign_x[i] = (xi < 0.0) as i32;
            }
            s
        }
    };
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    let sum = {
        let mut s = 0.0f32;
        for i in 0..n {
            let xi = x[i];
            let abs_xi = xi.abs();
            abs_x[i] = abs_xi;
            s += abs_xi;
            sign_x[i] = (xi < 0.0) as i32;
        }
        s
    };

    // The initialisers above fill the first `n` entries; trim to that once so
    // every index below is provably in range without a raw pointer to assert it.
    let abs_x = &abs_x[..n];
    let y2f = &mut y2f[..n];
    let sign_x = &sign_x[..n];
    let y = &mut y[..n];

    if k > (n >> 1) as i32 && sum > 1e-15 {
        let rcp = (k as f32 + 0.8) / sum;

        for i in 0..n {
            let yi = (abs_x[i] * rcp) as i32;
            y[i] = yi;
            let yf = yi as f32;
            yy += yf * yf;
            xy += yf * abs_x[i];
            y2f[i] = 2.0 * yf;
            k -= yi;
        }

        if k > n as i32 + 3 {
            let tmp = k as f32;
            yy += tmp * tmp;
            yy += tmp * y[0] as f32;
            y[0] += k;
            y2f[0] = 2.0 * y[0] as f32;
            k = 0;
        }
    }

    while k > 0 {
        yy += 1.0;

        let rxy0 = xy + abs_x[0];
        let mut best_id = 0usize;
        let mut best_num = rxy0 * rxy0;
        let mut best_den = yy + y2f[0];

        let mut i = 1;
        while i < n {
            let rxy = xy + abs_x[i];
            let ryy = yy + y2f[i];
            let rxy_sq = rxy * rxy;

            if best_den * rxy_sq > ryy * best_num {
                best_num = rxy_sq;
                best_den = ryy;
                best_id = i;
            }
            i += 1;
        }

        xy += abs_x[best_id];
        yy += y2f[best_id];
        y2f[best_id] += 2.0;
        y[best_id] += 1;
        k -= 1;
    }

    for i in 0..n {
        let s = sign_x[i];
        y[i] = (y[i] ^ -s) + s;
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2,fma")]
#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn pvq_search_avx2(x: &[f32], y: &mut [i32], k: i32, n: usize) {
    use std::arch::x86_64::*;

    // Same 31-sample bound as `pvq_search_scalar`, and for the same reason:
    // the working buffers below are fixed at 32 entries and the eight-wide
    // stores write straight into them. A `debug_assert!` compiles out of
    // release, which is where this would corrupt memory.
    assert!(
        n <= 31,
        "pvq_search_avx2: n = {n} exceeds its 31-sample bound"
    );
    debug_assert!(k > 4);

    let x = &x[..n];
    let y = &mut y[..n];

    let mut k = k;
    let mut yy = 0.0f32;
    let mut xy = 0.0f32;

    y[..n].fill(0);

    let mut abs_x = [0.0f32; 32];
    let mut y2f = [0.0f32; 32];
    let mut sign_x = [0i32; 32];

    let sign_mask = _mm256_set1_ps(-0.0f32);
    let vzero_ps = _mm256_setzero_ps();
    let vone_i = _mm256_set1_epi32(1);
    let mut acc = _mm256_setzero_ps();
    let mut i = 0;
    while i + 8 <= n {
        let v = _mm256_loadu_ps(x.as_ptr().add(i));
        let a = _mm256_andnot_ps(sign_mask, v);
        _mm256_storeu_ps(abs_x.as_mut_ptr().add(i), a);
        acc = _mm256_add_ps(acc, a);

        let neg_mask = _mm256_cmp_ps(v, vzero_ps, _CMP_LT_OS);
        let sign_i = _mm256_and_si256(_mm256_castps_si256(neg_mask), vone_i);
        _mm256_storeu_si256(sign_x.as_mut_ptr().add(i) as *mut __m256i, sign_i);
        i += 8;
    }

    let lo4 = _mm256_castps256_ps128(acc);
    let hi4 = _mm256_extractf128_ps(acc, 1);
    let s4 = _mm_add_ps(lo4, hi4);
    let s2 = _mm_add_ps(s4, _mm_movehl_ps(s4, s4));
    let s1 = _mm_add_ss(s2, _mm_shuffle_ps(s2, s2, 1));
    let mut sum = _mm_cvtss_f32(s1);
    for j in i..n {
        let xi = x[j];
        let a = xi.abs();
        abs_x[j] = a;
        sum += a;
        sign_x[j] = (xi < 0.0) as i32;
    }

    if k > (n >> 1) as i32 && sum > 1e-15 {
        let rcp = (k as f32 + 0.8) / sum;
        let vrcp = _mm256_set1_ps(rcp);
        let mut vyy_acc = _mm256_setzero_ps();
        let mut vxy_acc = _mm256_setzero_ps();
        let mut vk_acc = _mm256_setzero_ps();
        let mut i = 0;
        while i + 8 <= n {
            let vabs = _mm256_loadu_ps(abs_x.as_ptr().add(i));
            let vyi_f32 = _mm256_mul_ps(vabs, vrcp);

            let vyi_i = _mm256_cvttps_epi32(vyi_f32);
            _mm256_storeu_si256(y.as_mut_ptr().add(i) as *mut __m256i, vyi_i);
            let vyi_f = _mm256_cvtepi32_ps(vyi_i);

            vyy_acc = _mm256_fmadd_ps(vyi_f, vyi_f, vyy_acc);
            vxy_acc = _mm256_fmadd_ps(vyi_f, vabs, vxy_acc);
            vk_acc = _mm256_add_ps(vk_acc, vyi_f);

            let vy2f = _mm256_add_ps(vyi_f, vyi_f);
            _mm256_storeu_ps(y2f.as_mut_ptr().add(i), vy2f);
            i += 8;
        }

        let hsum = |v: __m256| -> f32 {
            let lo = _mm256_castps256_ps128(v);
            let hi = _mm256_extractf128_ps(v, 1);
            let s4 = _mm_add_ps(lo, hi);
            let s2 = _mm_add_ps(s4, _mm_movehl_ps(s4, s4));
            let s1 = _mm_add_ss(s2, _mm_shuffle_ps(s2, s2, 1));
            _mm_cvtss_f32(s1)
        };
        yy += hsum(vyy_acc);
        xy += hsum(vxy_acc);
        k -= hsum(vk_acc) as i32;

        while i < n {
            let yi = (abs_x[i] * rcp) as i32;
            y[i] = yi;
            let yf = yi as f32;
            yy += yf * yf;
            xy += yf * abs_x[i];
            y2f[i] = 2.0 * yf;
            k -= yi;
            i += 1;
        }
        if k > n as i32 + 3 {
            let tmp = k as f32;
            yy += tmp * tmp + tmp * y[0] as f32;
            y[0] += k;
            y2f[0] = 2.0 * y[0] as f32;
            k = 0;
        }
    }

    let abs_x_ptr = abs_x.as_ptr();
    let y2f_ptr = y2f.as_mut_ptr();
    let y_ptr = y.as_mut_ptr();
    let n8 = n & !7;
    let n_ceil8 = (n + 7) & !7;
    let mut scores = [0.0f32; 32];

    while k > 0 {
        yy += 1.0;
        let vxy = _mm256_set1_ps(xy);
        let vyy = _mm256_set1_ps(yy);

        let mut vmax = _mm256_setzero_ps();
        let mut j = 0;
        while j < n8 {
            let vabs = _mm256_loadu_ps(abs_x_ptr.add(j));
            let vy2f = _mm256_loadu_ps(y2f_ptr.add(j));
            let rxy = _mm256_add_ps(vabs, vxy);
            let ryy = _mm256_add_ps(vy2f, vyy);
            let score = _mm256_mul_ps(rxy, _mm256_rsqrt_ps(ryy));
            _mm256_storeu_ps(scores.as_mut_ptr().add(j), score);
            vmax = _mm256_max_ps(vmax, score);
            j += 8;
        }

        while j < n {
            let rxy = xy + *abs_x_ptr.add(j);
            let ryy = yy + *y2f_ptr.add(j);
            scores[j] = rxy * (1.0 / ryy.sqrt());
            j += 1;
        }

        let global_max = {
            let hi = _mm256_extractf128_ps(vmax, 1);
            let lo = _mm256_castps256_ps128(vmax);
            let m4 = _mm_max_ps(lo, hi);
            let m2 = _mm_max_ps(m4, _mm_movehl_ps(m4, m4));
            let m1 = _mm_max_ss(m2, _mm_shuffle_ps(m2, m2, 1));
            _mm_cvtss_f32(m1)
        };

        let mut gmax = global_max;
        for j in n8..n {
            if scores[j] > gmax {
                gmax = scores[j];
            }
        }

        let vgmax = _mm256_set1_ps(gmax);
        let mut best_id: usize = 0;
        let mut j = 0;
        while j < n_ceil8 {
            let vs = _mm256_loadu_ps(scores.as_ptr().add(j));
            let mask = _mm256_movemask_ps(_mm256_cmp_ps(vs, vgmax, _CMP_EQ_OQ)) as u32;
            if mask != 0 {
                best_id = j + mask.trailing_zeros() as usize;
                break;
            }
            j += 8;
        }

        xy += *abs_x_ptr.add(best_id);
        yy += *y2f_ptr.add(best_id);
        *y2f_ptr.add(best_id) += 2.0;
        *y_ptr.add(best_id) += 1;
        k -= 1;
    }

    for i in 0..n {
        let s = sign_x[i];
        y[i] = (y[i] ^ -s) + s;
    }
}

#[inline]
fn exp_rotation1(x: &mut [f32], len: usize, stride: usize, c: f32, s: f32) {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        exp_rotation1_neon(x, len, stride, c, s);
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        exp_rotation1_scalar(x, len, stride, c, s);
    }
}

#[inline]
fn exp_rotation1_scalar(x: &mut [f32], len: usize, stride: usize, c: f32, s: f32) {
    let ms = -s;
    for i in 0..(len - stride) {
        let x1 = x[i];
        let x2 = x[i + stride];
        x[i + stride] = c * x2 + s * x1;
        x[i] = c * x1 + ms * x2;
    }
    if len >= 2 * stride {
        for i in (0..(len - 2 * stride)).rev() {
            let x1 = x[i];
            let x2 = x[i + stride];
            x[i + stride] = c * x2 + s * x1;
            x[i] = c * x1 + ms * x2;
        }
    }
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn exp_rotation1_neon(x: &mut [f32], len: usize, stride: usize, c: f32, s: f32) {
    // Both passes pair `x[j]` with `x[j + stride]` inside `x[..len]`, and the
    // loop bounds are written as `len - stride`, so a stride past `len` would
    // wrap the subtraction and run off the end.
    assert!(
        stride <= len,
        "exp_rotation1: stride {stride} exceeds len {len}"
    );
    let x = &mut x[..len];

    if stride < 4 {
        exp_rotation1_scalar(x, len, stride, c, s);
        return;
    }

    use std::arch::aarch64::*;

    let vc = vdupq_n_f32(c);
    let vs = vdupq_n_f32(s);

    // Forward pass: SIMD when we can load 4 contiguous elements
    let mut i = 0;
    while i + 4 <= len - stride {
        let vx1 = vld1q_f32(x.as_ptr().add(i));
        let vx2 = vld1q_f32(x.as_ptr().add(i + stride));

        // y1 = c*x1 - s*x2
        let vy1 = vfmsq_f32(vmulq_f32(vx1, vc), vs, vx2);
        // y2 = c*x2 + s*x1
        let vy2 = vfmaq_f32(vmulq_f32(vx2, vc), vs, vx1);

        vst1q_f32(x.as_mut_ptr().add(i), vy1);
        vst1q_f32(x.as_mut_ptr().add(i + stride), vy2);

        i += 4;
    }
    for j in i..(len - stride) {
        let x1 = x[j];
        let x2 = x[j + stride];
        x[j + stride] = c * x2 + s * x1;
        x[j] = c * x1 - s * x2;
    }

    if len >= 2 * stride {
        for j in (0..(len - 2 * stride)).rev() {
            let x1 = x[j];
            let x2 = x[j + stride];
            x[j + stride] = c * x2 + s * x1;
            x[j] = c * x1 - s * x2;
        }
    }
}

#[inline(always)]
pub fn exp_rotation(x: &mut [f32], length: usize, dir: i32, stride: usize, k: i32, spread: i32) {
    const SPREAD_FACTOR: [i32; 3] = [15, 10, 5];
    if 2 * k >= length as i32 || spread <= 0 || spread > 3 {
        return;
    }
    let factor = SPREAD_FACTOR[spread as usize - 1];
    let gain = (length as f32) / (length as f32 + factor as f32 * k as f32);
    let theta = 0.5 * gain * gain;
    let c = (0.5 * std::f32::consts::PI * theta).cos();
    let s = (0.5 * std::f32::consts::PI * theta).sin();

    let mut stride2 = 0;
    if length >= 8 * stride {
        stride2 = 1;
        while (stride2 * stride2 + stride2) * stride + (stride >> 2) < length {
            stride2 += 1;
        }
    }

    let block_len = length / stride;
    for i in 0..stride {
        let x_offset = i * block_len;
        let x_subset = &mut x[x_offset..x_offset + block_len];
        if dir < 0 {
            if stride2 != 0 {
                exp_rotation1(x_subset, block_len, stride2, s, c);
            }
            exp_rotation1(x_subset, block_len, 1, c, s);
        } else {
            exp_rotation1(x_subset, block_len, 1, c, -s);
            if stride2 != 0 {
                exp_rotation1(x_subset, block_len, stride2, s, -c);
            }
        }
    }
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn extract_collapse_mask_neon(iy: &[i32], n: usize, b: usize) -> u32 {
    use std::arch::aarch64::*;

    if b <= 1 {
        return 1;
    }
    let n0 = n / b;
    let mut collapse_mask = 0u32;

    for i in 0..b {
        let base = i * n0;
        let slice = &iy[base..base + n0];

        let mut any_nonzero = false;
        let n4 = n0 & !3;
        let mut j = 0;

        while j < n4 {
            let v = vld1q_s32(slice.as_ptr().add(j));

            let or_val = vorrq_s32(v, vextq_s32(v, v, 2));
            let or_val = vorrq_s32(or_val, vextq_s32(or_val, or_val, 1));
            if vgetq_lane_s32(or_val, 0) != 0 {
                any_nonzero = true;
                break;
            }
            j += 4;
        }

        if !any_nonzero {
            for j in j..n0 {
                if slice[j] != 0 {
                    any_nonzero = true;
                    break;
                }
            }
        }

        if any_nonzero {
            collapse_mask |= 1 << i;
        }
    }
    collapse_mask
}

#[inline(always)]
pub fn extract_collapse_mask(iy: &[i32], n: usize, b: usize) -> u32 {
    if b <= 1 {
        return 1;
    }

    #[cfg(target_arch = "aarch64")]
    unsafe {
        extract_collapse_mask_neon(iy, n, b)
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        let n0 = n / b;
        let mut collapse_mask = 0u32;
        for i in 0..b {
            let mut tmp = 0i32;
            let base = i * n0;
            for j in 0..n0 {
                tmp |= iy[base + j];
            }
            if tmp != 0 {
                collapse_mask |= 1 << i;
            }
        }
        collapse_mask
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2,fma")]
#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn alg_quant_resynth_avx2(y: &[i32], x: &mut [f32], n: usize, gain: f32) {
    use std::arch::x86_64::*;

    // The kernel reads `n` pulses and writes `n` samples; trimming to `n` here
    // is what puts the loads and stores below in bounds.
    let y = &y[..n];
    let x = &mut x[..n];

    let mut acc0 = _mm256_setzero_ps();
    let mut i = 0;

    while i + 8 <= n {
        let yi = _mm256_loadu_si256(y.as_ptr().add(i) as *const __m256i);
        let yf = _mm256_cvtepi32_ps(yi);
        _mm256_storeu_ps(x.as_mut_ptr().add(i), yf);
        acc0 = _mm256_fmadd_ps(yf, yf, acc0);
        i += 8;
    }

    let lo = _mm256_castps256_ps128(acc0);
    let hi = _mm256_extractf128_ps(acc0, 1);
    let s4 = _mm_add_ps(lo, hi);
    let s2 = _mm_add_ps(s4, _mm_movehl_ps(s4, s4));
    let s1 = _mm_add_ss(s2, _mm_shuffle_ps(s2, s2, 1));
    let mut ryy = _mm_cvtss_f32(s1);

    for j in i..n {
        let v = y[j] as f32;
        x[j] = v;
        ryy += v * v;
    }

    let g = gain / (1e-15f32 + ryy).sqrt();
    let vg = _mm256_set1_ps(g);

    i = 0;
    while i + 8 <= n {
        let v = _mm256_loadu_ps(x.as_ptr().add(i));
        _mm256_storeu_ps(x.as_mut_ptr().add(i), _mm256_mul_ps(v, vg));
        i += 8;
    }
    for j in i..n {
        x[j] *= g;
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn pvq_search_scalar_init_avx2(
    x: &[f32],
    n: usize,
    abs_x: &mut [f32; 32],
    sign_x: &mut [i32; 32],
) -> f32 {
    use std::arch::x86_64::*;

    // The kernel reads `n` inputs and writes `n` entries of each output; the
    // trims below are what put those stores inside the fixed-size buffers and
    // the loads inside `x`.
    let x = &x[..n];
    let abs_x = &mut abs_x[..n];
    let sign_x = &mut sign_x[..n];
    let sign_mask = _mm256_set1_ps(-0.0f32);
    let mut acc = _mm256_setzero_ps();
    let mut i = 0;

    while i + 8 <= n {
        let v = _mm256_loadu_ps(x.as_ptr().add(i));
        let a = _mm256_andnot_ps(sign_mask, v);
        _mm256_storeu_ps(abs_x.as_mut_ptr().add(i), a);
        acc = _mm256_add_ps(acc, a);

        for j in 0..8 {
            sign_x[i + j] = (x[i + j] < 0.0) as i32;
        }
        i += 8;
    }

    let lo = _mm256_castps256_ps128(acc);
    let hi = _mm256_extractf128_ps(acc, 1);
    let s4 = _mm_add_ps(lo, hi);
    let s2 = _mm_add_ps(s4, _mm_movehl_ps(s4, s4));
    let s1 = _mm_add_ss(s2, _mm_shuffle_ps(s2, s2, 1));
    let mut sum = _mm_cvtss_f32(s1);

    for j in i..n {
        let abs_xi = x[j].abs();
        abs_x[j] = abs_xi;
        sum += abs_xi;
        sign_x[j] = (x[j] < 0.0) as i32;
    }
    sum
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn alg_quant_resynth_neon(y: &[i32], x: &mut [f32], n: usize, gain: f32) {
    use std::arch::aarch64::*;

    // The kernel reads `n` pulses and writes `n` samples; trimming to `n` here
    // is what puts the loads and stores below in bounds.
    let y = &y[..n];
    let x = &mut x[..n];

    let mut sum_vec = vdupq_n_f32(0.0);
    let n8 = n & !7;
    let mut i = 0;

    while i < n8 {
        let yi0 = vld1q_s32(y.as_ptr().add(i));
        let yi1 = vld1q_s32(y.as_ptr().add(i + 4));

        let yf0 = vcvtq_f32_s32(yi0);
        let yf1 = vcvtq_f32_s32(yi1);

        vst1q_f32(x.as_mut_ptr().add(i), yf0);
        vst1q_f32(x.as_mut_ptr().add(i + 4), yf1);

        sum_vec = vfmaq_f32(sum_vec, yf0, yf0);
        sum_vec = vfmaq_f32(sum_vec, yf1, yf1);

        i += 8;
    }

    let mut ryy = vaddvq_f32(sum_vec);
    for j in i..n {
        let v = y[j] as f32;
        x[j] = v;
        ryy += v * v;
    }

    let g = gain / (1e-15 + ryy).sqrt();
    let vg = vdupq_n_f32(g);

    i = 0;
    while i < n8 {
        let vx0 = vld1q_f32(x.as_ptr().add(i));
        let vx1 = vld1q_f32(x.as_ptr().add(i + 4));
        let vr0 = vmulq_f32(vx0, vg);
        let vr1 = vmulq_f32(vx1, vg);
        vst1q_f32(x.as_mut_ptr().add(i), vr0);
        vst1q_f32(x.as_mut_ptr().add(i + 4), vr1);
        i += 8;
    }

    for j in i..n {
        x[j] *= g;
    }
}

#[cfg(not(target_arch = "aarch64"))]
#[inline(always)]
fn alg_quant_resynth_scalar(y: &[i32], x: &mut [f32], n: usize, gain: f32) {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        if have_avx2_fma() {
            alg_quant_resynth_avx2(y, x, n, gain);
            return;
        }
    }
    let mut ryy = 0.0f32;
    for i in 0..n {
        let v = y[i] as f32;
        x[i] = v;
        ryy += v * v;
    }
    let g = gain / (1e-15 + ryy).sqrt();
    for i in 0..n {
        x[i] *= g;
    }
}

#[inline]
/// `y` is scratch for the pulse vector, supplied by the caller and at least
/// [`MAX_PVQ_N`] long. The reference takes it from the stack allocator
/// (`ALLOC(iy, N, int)`), which costs nothing; a fresh `[0i32; MAX_PVQ_N]` per
/// call costs a 1.4 KB `memset` that the pulse decoder immediately overwrites,
/// and PVQ runs once per partition per band per channel per frame. Profiled
/// against libopus on a fullband stereo decode, that zeroing alone was a
/// twentieth of the whole decode.
pub fn alg_quant(
    y: &mut [i32],
    x: &mut [f32],
    n: usize,
    k: i32,
    spread: i32,
    stride: usize,
    rc: &mut RangeCoder,
    gain: f32,
    resynth: bool,
) -> u32 {
    let y = &mut y[..n];

    exp_rotation(x, n, 1, stride, k, spread);
    pvq_search(x, y, k, n);
    let mask = extract_collapse_mask(y, n, stride);
    encode_pulses(y, n as u32, k as u32, rc);

    if resynth {
        #[cfg(target_arch = "aarch64")]
        unsafe {
            alg_quant_resynth_neon(y, x, n, gain);
        }
        #[cfg(not(target_arch = "aarch64"))]
        alg_quant_resynth_scalar(y, x, n, gain);
        exp_rotation(x, n, -1, stride, k, spread);
    }
    mask
}

#[inline]
/// `y` is scratch for the pulse vector; see [`alg_quant`] for why the caller
/// owns it.
pub fn alg_unquant(
    y: &mut [i32],
    x: &mut [f32],
    n: usize,
    k: i32,
    spread: i32,
    stride: usize,
    rc: &mut RangeCoder,
    gain: f32,
) -> u32 {
    decode_pulses(&mut y[..n], n as u32, k as u32, rc);

    let mask = extract_collapse_mask(&y[..n], n, stride);

    #[cfg(target_arch = "aarch64")]
    unsafe {
        alg_quant_resynth_neon(&y[..n], x, n, gain);
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        alg_quant_resynth_scalar(&y[..n], x, n, gain);
    }

    exp_rotation(x, n, -1, stride, k, spread);

    mask
}

// ---- SIMD kernels vs their scalar definitions ---------------------------
//
// The PVQ search runs on every encoded band, so a vectorised path that
// disagrees with the scalar one either corrupts the codeword or silently
// costs quality. Neither shows up in an end-to-end round trip, because the
// decoder reconstructs whatever the encoder chose.
#[cfg(test)]
mod simd_tests {
    use super::*;

    fn noise(n: usize, seed: u32) -> Vec<f32> {
        let mut s = seed | 1;
        (0..n)
            .map(|_| {
                s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (s >> 8) as f32 / (1 << 23) as f32 - 1.0
            })
            .collect()
    }

    /// libopus's search objective: maximise `xy^2 / yy`.
    fn score(x: &[f32], y: &[i32], n: usize) -> f32 {
        let mut xy = 0.0f32;
        let mut yy = 0.0f32;
        for i in 0..n {
            let yf = y[i] as f32;
            xy += x[i] * yf;
            yy += yf * yf;
        }
        if yy == 0.0 { 0.0 } else { xy * xy / yy }
    }

    /// A from-scratch transcription of libopus `op_pvq_search_c` (celt/vq.c),
    /// float build. This is the definition every path in this module is meant
    /// to implement, so it is written here rather than borrowed from any of
    /// them.
    fn pvq_search_reference(x: &[f32], k: i32, n: usize) -> Vec<i32> {
        let mut signx = vec![false; n];
        let mut xa = vec![0.0f32; n];
        let mut iy = vec![0i32; n];
        let mut y = vec![0.0f32; n];
        for j in 0..n {
            signx[j] = x[j] < 0.0;
            xa[j] = x[j].abs();
        }

        let mut xy = 0.0f32;
        let mut yy = 0.0f32;
        let mut pulses_left = k;

        if k > (n >> 1) as i32 {
            let mut sum: f32 = xa.iter().sum();
            // "Prevents infinities and NaNs from causing too many pulses to be
            // allocated. 64 is an approximation of infinity here."
            if !(sum > 1e-15 && sum < 64.0) {
                xa[0] = 1.0;
                for v in xa[1..n].iter_mut() {
                    *v = 0.0;
                }
                sum = 1.0;
            }
            let rcp = (k as f32 + 0.8) / sum;
            for j in 0..n {
                iy[j] = (rcp * xa[j]).floor() as i32;
                y[j] = iy[j] as f32;
                yy += y[j] * y[j];
                xy += xa[j] * y[j];
                y[j] *= 2.0;
                pulses_left -= iy[j];
            }
        }

        if pulses_left > n as i32 + 3 {
            let tmp = pulses_left as f32;
            yy += tmp * tmp;
            yy += tmp * y[0];
            iy[0] += pulses_left;
            pulses_left = 0;
        }

        for _ in 0..pulses_left {
            yy += 1.0;
            let rxy = xy + xa[0];
            let mut best_num = rxy * rxy;
            let mut best_den = yy + y[0];
            let mut best_id = 0usize;
            for j in 1..n {
                let rxy = xy + xa[j];
                let rxy = rxy * rxy;
                let ryy = yy + y[j];
                if best_den * rxy > ryy * best_num {
                    best_den = ryy;
                    best_num = rxy;
                    best_id = j;
                }
            }
            xy += xa[best_id];
            yy += y[best_id];
            y[best_id] += 2.0;
            iy[best_id] += 1;
        }

        for j in 0..n {
            if signx[j] {
                iy[j] = -iy[j];
            }
        }
        iy
    }

    const GRID: &[(usize, i32)] = &[
        (2, 1),
        (2, 5),
        (4, 1),
        (4, 3),
        (4, 12),
        (8, 2),
        (8, 7),
        (8, 40),
        (12, 5),
        (16, 1),
        (16, 6),
        (16, 30),
        (24, 9),
        (31, 11),
        (32, 8),
        (48, 15),
        (64, 20),
        (176, 32),
    ];

    /// Whatever path the dispatcher takes, the codeword must spend exactly `k`
    /// pulses. A search that returns the wrong pulse count produces an index
    /// the decoder cannot reconstruct.
    #[test]
    fn pvq_search_spends_exactly_k_pulses() {
        for &(n, k) in GRID {
            for seed in 0..4u32 {
                let x = noise(n, 0x9001 ^ (n as u32) ^ (seed << 16));
                let mut y = vec![0i32; n.max(MAX_PVQ_N.min(n))];
                pvq_search(&x, &mut y, k, n);
                let spent: i32 = y[..n].iter().map(|v| v.abs()).sum();
                assert_eq!(spent, k, "pvq_search n={n} k={k} seed={seed} spent {spent}");
            }
        }
    }

    /// Below n = 32, outside the two closed forms, the search must reproduce
    /// libopus exactly — no approximation, no tie-break drift.
    ///
    /// This is the test that caught the NEON kernel: it chose a different
    /// codeword in 470 of 1880 cases and a worse-scoring one in 450, giving up
    /// as much as 29.7% of the search objective, while every end-to-end test
    /// passed. The decoder reconstructs whatever the encoder picked, so a bad
    /// search costs quality silently and shows up nowhere else.
    #[test]
    fn pvq_search_matches_the_libopus_reference() {
        for &(n, k) in GRID {
            if n == 2 || n == 4 || n >= 32 {
                continue; // covered by the pinned-gap test below
            }
            for seed in 0..8u32 {
                let x = noise(n, 0xA001 ^ (n as u32) ^ (seed << 16));
                let mut got = vec![0i32; n];
                pvq_search(&x, &mut got, k, n);
                let want = pvq_search_reference(&x, k, n);
                assert_eq!(got, want, "pvq_search n={n} k={k} seed={seed}");
            }
        }
    }

    /// Three paths deliberately trade accuracy for speed: the n = 2 and n = 4
    /// closed forms, and the fast-select search used from n = 32 up. They are
    /// the same on every architecture, and none of them is free — measured
    /// against the reference they give up at most 1.7%, 3.3% and 2.0% of the
    /// objective respectively. Pinned here so the trade cannot quietly widen.
    #[test]
    fn pvq_search_approximations_stay_within_their_measured_gap() {
        const BUDGET: f32 = 0.05;
        for n in [2usize, 4, 32, 40, 48, 64, 96, 176] {
            for k in 1..=48i32 {
                for seed in 0..8u32 {
                    let x = noise(n, 0xA101 ^ (n as u32) ^ ((k as u32) << 8) ^ (seed << 20));
                    let mut got = vec![0i32; n];
                    pvq_search(&x, &mut got, k, n);
                    let want = pvq_search_reference(&x, k, n);
                    let (sg, sw) = (score(&x, &got, n), score(&x, &want, n));
                    assert!(
                        sg >= sw * (1.0 - BUDGET) - 1e-6,
                        "pvq_search n={n} k={k} seed={seed}: scored {sg} against {sw}, \
                         {:.2}% below the reference",
                        100.0 * (sw - sg) / sw
                    );
                }
            }
        }
    }

    /// The fast-select search's setup pass — absolute values, signs and their
    /// sum — has a NEON kernel. The search around it is an approximation, so a
    /// bug here would hide inside that tolerance; check it directly.
    #[cfg(target_arch = "aarch64")]
    #[test]
    fn simd_pvq_fast_select_init_matches_the_scalar_definition() {
        for &n in &[1usize, 3, 4, 7, 8, 15, 16, 32, 64, 176, 352] {
            let x = noise(n, 0xF001 ^ n as u32);
            let mut abs_mu = [0.0f32; MAX_PVQ_N];
            let mut sign_mu = [0i32; MAX_PVQ_N];
            let got_sum = unsafe { pvq_fast_select_init_neon(&x, n, &mut abs_mu, &mut sign_mu) };

            let mut want_sum = 0.0f32;
            for i in 0..n {
                let a = abs_mu[i];
                let sg = sign_mu[i];
                assert_eq!(a, x[i].abs(), "abs at {i} (n={n})");
                assert_eq!(sg, if x[i] < 0.0 { -1 } else { 1 }, "sign at {i} (n={n})");
                want_sum += x[i].abs();
            }
            assert!(
                (got_sum - want_sum).abs() <= 1e-4 * (1.0 + want_sum),
                "pvq_fast_select_init sum n={n}: {got_sum} vs {want_sum}"
            );
        }
    }

    #[test]
    fn simd_exp_rotation1_matches_the_scalar_definition() {
        for &len in &[4usize, 8, 15, 16, 33, 64, 176] {
            for &stride in &[1usize, 2, 3, 4, 8] {
                if len <= stride {
                    continue;
                }
                let src = noise(len, 0xC001 ^ (len as u32) ^ ((stride as u32) << 16));
                let (c, s) = (0.923_88_f32, 0.382_68_f32);

                let mut got = src.clone();
                exp_rotation1(&mut got, len, stride, c, s);
                let mut want = src.clone();
                exp_rotation1_scalar(&mut want, len, stride, c, s);

                let scale = want.iter().fold(1.0f32, |m, v| m.max(v.abs()));
                for (i, (g, w)) in got.iter().zip(&want).enumerate() {
                    assert!(
                        (g - w).abs() <= 1e-5 * scale,
                        "exp_rotation1 len={len} stride={stride} element {i}: {g} vs {w}"
                    );
                }
            }
        }
    }

    #[test]
    fn simd_extract_collapse_mask_matches_the_scalar_definition() {
        for &b in &[1usize, 2, 4, 8] {
            for &n0 in &[1usize, 2, 3, 5, 8, 16] {
                let n = n0 * b;
                for seed in 0..6u32 {
                    // A sparse vector so whole blocks really do collapse.
                    let mut iy = vec![0i32; n];
                    let mut st = (seed | 1).wrapping_mul(2_654_435_761);
                    for v in iy.iter_mut() {
                        st = st.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                        *v = if st >> 29 == 0 {
                            (st as i32 % 7) - 3
                        } else {
                            0
                        };
                    }
                    let got = extract_collapse_mask(&iy, n, b);

                    let want = if b <= 1 {
                        1
                    } else {
                        let mut mask = 0u32;
                        for i in 0..b {
                            let mut tmp = 0i32;
                            for j in 0..n0 {
                                tmp |= iy[i * n0 + j];
                            }
                            if tmp != 0 {
                                mask |= 1 << i;
                            }
                        }
                        mask
                    };
                    assert_eq!(got, want, "extract_collapse_mask n={n} b={b} seed={seed}");
                }
            }
        }
    }

    #[test]
    fn simd_alg_quant_resynth_matches_the_scalar_definition() {
        for &n in &[1usize, 4, 7, 8, 15, 16, 33, 176] {
            for &gain in &[1.0f32, 0.5, 3.0] {
                let mut y = vec![0i32; n];
                let mut st = 0xD001u32 ^ (n as u32);
                for v in y.iter_mut() {
                    st = st.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    *v = (st as i32 % 9) - 4;
                }

                let mut got = vec![0.0f32; n];
                #[cfg(target_arch = "aarch64")]
                unsafe {
                    alg_quant_resynth_neon(&y, &mut got, n, gain)
                };
                #[cfg(not(target_arch = "aarch64"))]
                alg_quant_resynth_scalar(&y, &mut got, n, gain);

                let mut ryy = 0.0f32;
                let mut want = vec![0.0f32; n];
                for i in 0..n {
                    let v = y[i] as f32;
                    want[i] = v;
                    ryy += v * v;
                }
                let g = gain / (1e-15 + ryy).sqrt();
                for v in want.iter_mut() {
                    *v *= g;
                }

                let scale = want.iter().fold(1.0f32, |m, v| m.max(v.abs()));
                for (i, (a, b)) in got.iter().zip(&want).enumerate() {
                    assert!(
                        (a - b).abs() <= 1e-5 * scale,
                        "alg_quant_resynth n={n} gain={gain} element {i}: {a} vs {b}"
                    );
                }
            }
        }
    }
}
