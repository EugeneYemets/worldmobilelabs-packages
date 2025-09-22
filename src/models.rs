use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct Country {
    pub code: String,
    pub name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct CountryListResponse {
    pub countries: Vec<Country>,
    pub count: usize,
    /// worldmobile | stub | fail_open_stub
    pub source: String,
}

#[derive(Debug, Deserialize, ToSchema, Default, Clone, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct CountryQuery {
    pub country_code: Option<String>,
    pub scope: Option<String>,
    pub esim_id: Option<String>,
    /// пройти всі сторінки
    pub fetch_all: Option<bool>,
    /// бажаний розмір сторінки (якщо підтримується)
    pub page_size: Option<u32>,
}
