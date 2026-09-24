use crate::liquid_cheatsheets;

pub(crate) fn main() -> anyhow::Result<()> {
    liquid_cheatsheets::run(liquid_cheatsheets::Case::Segmented)
}
