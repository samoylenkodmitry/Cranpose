// The sort chips of a collection and what each asks the API for.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum Sort {
    #[default]
    Featured,
    BestSelling,
    PriceAscending,
    PriceDescending,
}

impl Sort {
    pub(crate) const ALL: [Self; 4] = [
        Self::Featured,
        Self::BestSelling,
        Self::PriceAscending,
        Self::PriceDescending,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Featured => "Featured",
            Self::BestSelling => "Best selling",
            Self::PriceAscending => "Price: low to high",
            Self::PriceDescending => "Price: high to low",
        }
    }

    pub(crate) fn tag(self) -> &'static str {
        match self {
            Self::Featured => "sort-featured",
            Self::BestSelling => "sort-best-selling",
            Self::PriceAscending => "sort-price-asc",
            Self::PriceDescending => "sort-price-desc",
        }
    }

    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Featured => "COLLECTION_DEFAULT",
            Self::BestSelling => "BEST_SELLING",
            Self::PriceAscending | Self::PriceDescending => "PRICE",
        }
    }

    pub(crate) fn reverse(self) -> bool {
        self == Self::PriceDescending
    }
}
