use std::{cmp::Ordering, ops::Range};

use crate::{
    font::{FontFamily, FontStyle, FontWeight},
    font_fallback::{FallbackChain, draws_plainly},
    typeface::{FontFamilyKey, Typeface},
};

/// The family, weight and style text asks of a typeface.
#[derive(Clone, Copy, Debug)]
pub struct FontRequest<'a> {
    /// The family to draw with. `FontFamily::Default`, and a family no
    /// typeface belongs to, draw with the families in the order they were
    /// given.
    pub family: &'a FontFamily,
    /// The weight to draw with.
    pub weight: FontWeight,
    /// The style to draw with.
    pub style: FontStyle,
}

/// The typefaces text resolves against, grouped into families: Compose's
/// `FontFamily.Resolver`.
///
/// Typefaces registered under one `FontFamily`, or read from fonts that name
/// the same family, form one family. A request draws with the requested
/// family first, then with every other family in the order its first typeface
/// was given; within a family it takes the typefaces Compose's `FontMatcher`
/// picks for the weight and style. As in Android's text stack, each character
/// of a text goes to the first of these typefaces that maps it, the marks and
/// joiners of a cluster go with their base, and punctuation and spaces stay
/// in the typeface of the word before them when that typeface has them; emoji
/// go to color typefaces first, other text to plain ones.
pub struct FontFamilyResolver {
    // Grouped by family, the families in the order their first typeface came.
    typefaces: Box<[Typeface]>,
    families: Box<[Family]>,
}

struct Family {
    typefaces: Range<usize>,
    registered: Option<FontFamilyKey>,
    // The closest typeface to each weight of a whole hundred from 100 to
    // 900, upright then italic: what most text asks for, scored once.
    closest: [usize; 18],
}

// The weights `closest` keeps the typefaces of.
const HUNDREDS: [u16; 9] = [100, 200, 300, 400, 500, 600, 700, 800, 900];

// Where `closest` keeps the typeface of `weight` and `style`, when the
// weight is a whole hundred from 100 to 900.
fn closest_slot(weight: u16, style: FontStyle) -> Option<usize> {
    let hundreds = usize::from(weight / 100);
    (weight % 100 == 0 && (1..=9).contains(&hundreds))
        .then(|| hundreds - 1 + 9 * usize::from(style == FontStyle::Italic))
}

// The index of the first of the numbered typefaces of a family closest to
// `weight` and `style`, each scored once.
fn closest<'a>(
    members: impl Iterator<Item = (usize, &'a Typeface)>,
    weight: u16,
    style: FontStyle,
) -> usize {
    let requested = f32::from(weight);
    members
        .map(|(index, typeface)| (MatchScore::of(typeface, requested, style), index))
        .min_by(|a, b| a.0.cmp(b.0))
        .map_or(0, |(_, index)| index)
}

impl FontFamilyResolver {
    /// Groups `typefaces` into families, keeping the order in which each
    /// family's first typeface comes.
    pub fn new(typefaces: impl IntoIterator<Item = Typeface>) -> Self {
        let mut numbered: Vec<(usize, Typeface)> = Vec::new();
        let mut first_of_family: Vec<usize> = Vec::new();
        for typeface in typefaces {
            let family = first_of_family
                .iter()
                .position(|&first| same_family(&numbered[first].1, &typeface))
                .unwrap_or_else(|| {
                    first_of_family.push(numbered.len());
                    first_of_family.len() - 1
                });
            numbered.push((family, typeface));
        }
        numbered.sort_by_key(|(family, _)| *family);
        let mut families = Vec::with_capacity(first_of_family.len());
        let mut start = 0;
        for members in numbered.chunk_by(|a, b| a.0 == b.0) {
            let numbered = || (start..).zip(members.iter().map(|(_, typeface)| typeface));
            families.push(Family {
                typefaces: start..start + members.len(),
                registered: members[0].1.registered_family(),
                closest: std::array::from_fn(|slot| {
                    let style = if slot < 9 {
                        FontStyle::Normal
                    } else {
                        FontStyle::Italic
                    };
                    closest(numbered(), HUNDREDS[slot % 9], style)
                }),
            });
            start += members.len();
        }
        Self {
            typefaces: numbered.into_iter().map(|(_, typeface)| typeface).collect(),
            families: families.into_boxed_slice(),
        }
    }

    /// Every typeface, grouped by family.
    pub fn typefaces(&self) -> &[Typeface] {
        &self.typefaces
    }

    /// The typeface that draws text of `request` where it covers the text,
    /// and sets an empty line's height. `None` when the resolver has no
    /// typefaces.
    pub fn resolve(&self, request: FontRequest<'_>) -> Option<&Typeface> {
        self.primary(request).map(|index| &self.typefaces[index])
    }

    /// Splits `text` into runs that one typeface draws, in order, and passes
    /// each run's byte range and typeface to `visit`.
    pub fn runs<'a>(
        &'a self,
        text: &str,
        request: FontRequest<'_>,
        mut visit: impl FnMut(Range<usize>, &'a Typeface),
    ) {
        self.typeface_runs(text, request, |range, typeface| {
            visit(range, &self.typefaces[typeface]);
        });
    }

    // `runs` with each typeface as its index in `typefaces`.
    pub(crate) fn typeface_runs(
        &self,
        text: &str,
        request: FontRequest<'_>,
        mut visit: impl FnMut(Range<usize>, usize),
    ) {
        let Some(primary) = self.primary(request) else {
            return;
        };
        if text.is_empty() {
            return;
        }
        let primary_face = &self.typefaces[primary];
        if self.typefaces.len() == 1
            || (!primary_face.is_color() && draws_plainly(primary_face.coverage(), text))
        {
            visit(0..text.len(), primary);
            return;
        }
        self.fallback_chain(request).split(text, visit);
    }

    fn primary(&self, request: FontRequest<'_>) -> Option<usize> {
        let wanted = Wanted::of(request.family);
        let family = self
            .families
            .iter()
            .find(|family| wanted.matches(self, family))
            .or_else(|| self.families.first())?;
        let weight = request.weight.value();
        Some(match closest_slot(weight, request.style) {
            Some(slot) => family.closest[slot],
            None => closest(
                family
                    .typefaces
                    .clone()
                    .map(|index| (index, &self.typefaces[index])),
                weight,
                request.style,
            ),
        })
    }

    // The closest typefaces of each family in fallback order, the requested
    // families first.
    fn fallback_chain(&self, request: FontRequest<'_>) -> FallbackChain<'_> {
        let wanted = Wanted::of(request.family);
        FallbackChain::new(
            [true, false]
                .into_iter()
                .flat_map(move |requested| {
                    self.families
                        .iter()
                        .filter(move |family| wanted.matches(self, family) == requested)
                })
                .flat_map(move |family| self.closest(family, request))
                .map(|typeface| (typeface, &self.typefaces[typeface])),
        )
    }

    // The typefaces of `family` that Compose's `FontMatcher` picks, in the
    // order given: the requested style first, then the requested weight, else
    // lighter ones below 400, heavier ones above 500, and up to 500 before
    // lighter, then heavier ones between. Several typefaces tie when they
    // split one weight and style between them by the characters they cover,
    // as CSS faces with a `unicode-range` do.
    fn closest(&self, family: &Family, request: FontRequest<'_>) -> impl Iterator<Item = usize> {
        let requested = f32::from(request.weight.value());
        let score =
            move |index: usize| MatchScore::of(&self.typefaces[index], requested, request.style);
        let best = family.typefaces.clone().map(score).min_by(|a, b| a.cmp(*b));
        family
            .typefaces
            .clone()
            .filter(move |&index| best.is_some_and(|best| score(index).cmp(best).is_eq()))
    }
}

fn same_family(a: &Typeface, b: &Typeface) -> bool {
    match (a.registered_family(), b.registered_family()) {
        (Some(a), Some(b)) => a == b,
        (None, None) => a
            .family_names()
            .first()
            .zip(b.family_names().first())
            .is_some_and(|(a, b)| a.eq_ignore_ascii_case(b)),
        _ => false,
    }
}

#[derive(Clone, Copy)]
enum Wanted<'a> {
    Any,
    Named { name: &'a str, key: FontFamilyKey },
    Registered(FontFamilyKey),
}

impl<'a> Wanted<'a> {
    fn of(family: &'a FontFamily) -> Self {
        match family {
            FontFamily::Default => Self::Any,
            FontFamily::Named(name) => Self::Named {
                name,
                key: FontFamilyKey::of(family),
            },
            _ => Self::Registered(FontFamilyKey::of(family)),
        }
    }

    fn matches(self, resolver: &FontFamilyResolver, family: &Family) -> bool {
        match self {
            Self::Any => true,
            Self::Named { name, key } => {
                family.registered == Some(key)
                    || resolver.typefaces[family.typefaces.clone()]
                        .iter()
                        .flat_map(Typeface::family_names)
                        .any(|family| family.eq_ignore_ascii_case(name))
            }
            Self::Registered(key) => family.registered == Some(key),
        }
    }
}

// How far a typeface is from a requested weight and style; less is closer.
#[derive(Clone, Copy)]
struct MatchScore {
    style_differs: bool,
    weight_group: u8,
    weight_distance: f32,
}

impl MatchScore {
    fn of(typeface: &Typeface, requested: f32, style: FontStyle) -> Self {
        let (lightest, heaviest) = typeface.weights();
        let weight = requested.clamp(lightest, heaviest);
        let weight_group = if requested < 400.0 {
            u8::from(weight > requested)
        } else if requested > 500.0 {
            u8::from(weight < requested)
        } else if weight < requested {
            1
        } else if weight <= 500.0 {
            0
        } else {
            2
        };
        Self {
            style_differs: !typeface.draws(style),
            weight_group,
            weight_distance: (weight - requested).abs(),
        }
    }

    fn cmp(self, other: Self) -> Ordering {
        (self.style_differs, self.weight_group)
            .cmp(&(other.style_differs, other.weight_group))
            .then(self.weight_distance.total_cmp(&other.weight_distance))
    }
}
