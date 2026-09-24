use cranpose::{
    liquid::prelude::{LiquidTabBar, LiquidTabBarScope, LiquidTabBarSpec},
    rememberMutableStateOf, Modifier,
};

pub fn tabs(
    entries: &'static [(&'static str, &'static str)],
) -> impl Fn(&LiquidTabBarScope) + 'static {
    move |scope: &LiquidTabBarScope| {
        for &(icon, label) in entries {
            scope.tab(icon, label);
        }
    }
}

pub fn bar(
    modifier: Modifier,
    spec: LiquidTabBarSpec,
    initial_selection: usize,
    entries: &'static [(&'static str, &'static str)],
) {
    let selected = rememberMutableStateOf(move || initial_selection);
    LiquidTabBar(
        modifier,
        spec,
        selected.get(),
        move |index| selected.set(index),
        tabs(entries),
    );
}
