//! Python addons/reqs2reqs/shuffle.py の設定契約。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {}

use std::hash::{BuildHasher, Hasher};
impl Config {
    /// seedはPython random.seed(int)の比較用。設定ファイルにseedキーは追加しない。
    pub fn apply(
        &self,
        mut requests: Vec<crate::models::Request>,
        seed: Option<u64>,
    ) -> Vec<crate::models::Request> {
        let seed = seed.unwrap_or_else(|| {
            std::collections::hash_map::RandomState::new()
                .build_hasher()
                .finish()
        });
        let mut random = PythonRandom::new(seed);
        for i in (1..requests.len()).rev() {
            requests.swap(i, random.below(i + 1));
        }
        requests
    }
}

// CPythonのMT19937、整数seedのinit_by_array、_randbelow(getrandbits)を再現する。
struct PythonRandom {
    state: [u32; 624],
    index: usize,
}
impl PythonRandom {
    fn new(seed: u64) -> Self {
        let mut result = Self {
            state: [0; 624],
            index: 624,
        };
        result.state[0] = 19_650_218;
        for i in 1..624 {
            result.state[i] = 1_812_433_253u32
                .wrapping_mul(result.state[i - 1] ^ (result.state[i - 1] >> 30))
                .wrapping_add(i as u32);
        }
        let words = [seed as u32, (seed >> 32) as u32];
        let key = if words[1] == 0 {
            &words[..1]
        } else {
            &words[..]
        };
        let (mut i, mut j) = (1, 0);
        for _ in 0..624 {
            result.state[i] = (result.state[i]
                ^ (result.state[i - 1] ^ (result.state[i - 1] >> 30)).wrapping_mul(1_664_525))
            .wrapping_add(key[j])
            .wrapping_add(j as u32);
            i += 1;
            j += 1;
            if i == 624 {
                result.state[0] = result.state[623];
                i = 1;
            }
            if j == key.len() {
                j = 0;
            }
        }
        for _ in 0..623 {
            result.state[i] = (result.state[i]
                ^ (result.state[i - 1] ^ (result.state[i - 1] >> 30)).wrapping_mul(1_566_083_941))
            .wrapping_sub(i as u32);
            i += 1;
            if i == 624 {
                result.state[0] = result.state[623];
                i = 1;
            }
        }
        result.state[0] = 0x8000_0000;
        result
    }
    fn next(&mut self) -> u32 {
        if self.index == 624 {
            for i in 0..624 {
                let y = (self.state[i] & 0x8000_0000) | (self.state[(i + 1) % 624] & 0x7fff_ffff);
                self.state[i] = self.state[(i + 397) % 624]
                    ^ (y >> 1)
                    ^ if y & 1 == 1 { 0x9908_b0df } else { 0 };
            }
            self.index = 0;
        }
        let mut y = self.state[self.index];
        self.index += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^ (y >> 18)
    }
    fn below(&mut self, n: usize) -> usize {
        let bits = usize::BITS - n.leading_zeros();
        loop {
            let value = if bits <= 32 {
                (self.next() >> (32 - bits)) as usize
            } else {
                self.next() as u64 as usize | ((self.next() >> (64 - bits)) as usize) << 32
            };
            if value < n {
                return value;
            }
        }
    }
}
