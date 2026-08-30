#![allow(dead_code)]

use cranpose::liquid::prelude::LiquidTabBarScope;

pub fn tabs(
    entries: &'static [(&'static str, &'static str)],
) -> impl Fn(&LiquidTabBarScope) + 'static {
    move |scope: &LiquidTabBarScope| {
        for &(icon, label) in entries {
            scope.tab(icon, label);
        }
    }
}
