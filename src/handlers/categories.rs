use crate::api::error::ApiError;
use crate::api::extractors::ApiPath;
use crate::api::response::ListResponse;
use crate::domain::category::{
    CategoryId, CategoryKind, CategoryName, MiseAJourCategorie, NouvelleCategorie,
    couleur_ou_defaut, icone_ou_defaut,
};
use crate::domain::compte::ProprietaireId;
use crate::domain::ports::ecriture::CategoriesWriteRepository;
use crate::domain::ports::lecture::CategoriesReadRepository;
use crate::extract::BudgyUser;
use crate::handlers::dto::{CategoryDto, CategoryRequest};
use crate::state::AppState;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use uuid::Uuid;

pub async fn list_categories(
    user: BudgyUser,
    State(state): State<AppState>,
) -> Result<Json<ListResponse<CategoryDto>>, ApiError> {
    let proprietaire = ProprietaireId(user.owner_id().to_string());
    let categories = state
        .categories
        .lister_pour_proprietaire(&proprietaire)
        .await?;
    let total = categories.len() as u64;
    let data = categories.into_iter().map(CategoryDto::from).collect();
    Ok(Json(ListResponse::new(data, total)))
}

pub async fn create_category(
    user: BudgyUser,
    State(state): State<AppState>,
    Json(payload): Json<CategoryRequest>,
) -> Result<(StatusCode, Json<CategoryDto>), ApiError> {
    let proprietaire = ProprietaireId(user.owner_id().to_string());
    let name = parse_name(&payload.name)?;
    let kind = parse_kind(&payload.kind)?;

    let categorie = state
        .categories
        .creer(NouvelleCategorie {
            proprietaire,
            name,
            kind,
            color: couleur_ou_defaut(payload.color),
            icon: icone_ou_defaut(payload.icon),
        })
        .await?;

    Ok((StatusCode::CREATED, Json(CategoryDto::from(categorie))))
}

pub async fn update_category(
    user: BudgyUser,
    State(state): State<AppState>,
    ApiPath(category_id): ApiPath<Uuid>,
    Json(payload): Json<CategoryRequest>,
) -> Result<Json<CategoryDto>, ApiError> {
    let proprietaire = ProprietaireId(user.owner_id().to_string());
    let name = parse_name(&payload.name)?;
    let kind = parse_kind(&payload.kind)?;

    let categorie = state
        .categories
        .mettre_a_jour(
            &proprietaire,
            &CategoryId(category_id),
            MiseAJourCategorie {
                name,
                kind,
                color: couleur_ou_defaut(payload.color),
                icon: icone_ou_defaut(payload.icon),
            },
        )
        .await?
        .ok_or_else(|| ApiError::not_found("catégorie introuvable"))?;

    Ok(Json(CategoryDto::from(categorie)))
}

pub async fn delete_category(
    user: BudgyUser,
    State(state): State<AppState>,
    ApiPath(category_id): ApiPath<Uuid>,
) -> Result<StatusCode, ApiError> {
    let proprietaire = ProprietaireId(user.owner_id().to_string());
    let supprimee = state
        .categories
        .supprimer(&proprietaire, &CategoryId(category_id))
        .await?;

    if !supprimee {
        return Err(ApiError::not_found("catégorie introuvable"));
    }

    Ok(StatusCode::NO_CONTENT)
}

fn parse_name(value: &str) -> Result<CategoryName, ApiError> {
    CategoryName::parse(value).map_err(|e| ApiError::validation(e.to_string()))
}

fn parse_kind(value: &str) -> Result<CategoryKind, ApiError> {
    CategoryKind::parse(value.trim()).ok_or_else(|| {
        ApiError::validation("type de catégorie invalide (revenu ou depense attendu)")
    })
}
