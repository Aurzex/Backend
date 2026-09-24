//! id 生成:KN 侧实体/程序集用 UUID 形态,影子和块 id 用小写短 id。
//!
//! 官方编辑器每次运行都现铸 UUID(`crypto.randomUUID`),因此同一作品两次转换的产物**不会**逐字节相同
//! (实测:同一输入两次运行有 28 个 id 不同)。为了能跟官方产物做逐字节对齐与往返测试,
//! [`IdSource::new(true)`](确定性模式)改用递增计数器生成固定形态的 id。

use crate::core::convert::shared::IdGenerator;

/// id 来源(确定性 / 随机)
#[derive(Clone)]
pub(crate) struct IdSource {
    deterministic: bool,
    counter: u64,
    chars: IdGenerator,
}

impl IdSource {
    pub(crate) fn new(deterministic: bool) -> Self {
        IdSource {
            deterministic,
            counter: 0,
            chars: IdGenerator::new(),
        }
    }

    /// UUID v4 形态(实体 / 程序集 / KN 影子块)
    pub(crate) fn uuid(&mut self) -> String {
        self.counter += 1;
        if self.deterministic {
            return format!("00000000-0000-4000-8000-{:012x}", self.counter);
        }
        let hex = |n: usize| -> String {
            (0..n)
                .map(|_| std::char::from_digit(fastrand::u32(0..16), 16).unwrap())
                .collect()
        };
        let variant = ['8', '9', 'a', 'b'][fastrand::usize(0..4)]; // RFC 4122 variant 位固定为 10xx(单字符)
        format!(
            "{}-{}-4{}-{}{}-{}",
            hex(8),
            hex(4),
            hex(3),
            variant,
            hex(3),
            hex(12)
        )
    }

    /// 22 字符短 id(与反编译侧 `IdGenerator` 同风格;KN 里两种形态都见得到)
    pub(crate) fn short(&mut self) -> String {
        self.counter += 1;
        if self.deterministic {
            return format!("{:0>22}", format!("{:x}", self.counter));
        }
        self.chars.generate(22)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn deterministic_ids_are_stable_and_unique() {
        let mut a = IdSource::new(true);
        let mut b = IdSource::new(true);
        let left: Vec<String> = (0..5).map(|_| a.uuid()).collect();
        let right: Vec<String> = (0..5).map(|_| b.uuid()).collect();
        assert_eq!(left, right, "同一序号序列必须一致");
        assert_eq!(left.len(), left.iter().collect::<HashSet<_>>().len());
        assert!(
            left.iter().all(|id| id.len() == 36),
            "UUID 形态:{}",
            left[0]
        );
    }

    #[test]
    fn random_ids_look_like_uuid_v4() {
        let mut src = IdSource::new(false);
        let id = src.uuid();
        let parts: Vec<&str> = id.split('-').collect();
        assert_eq!(
            parts.iter().map(|p| p.len()).collect::<Vec<_>>(),
            vec![8, 4, 4, 4, 12]
        );
        assert!(parts[2].starts_with('4'));
        assert!(id.chars().all(|c| c == '-' || c.is_ascii_hexdigit()));
    }

    #[test]
    fn short_ids_have_fixed_length() {
        let mut src = IdSource::new(false);
        assert_eq!(src.short().len(), 22);
        let mut det = IdSource::new(true);
        assert_eq!(det.short().len(), 22);
    }
}
