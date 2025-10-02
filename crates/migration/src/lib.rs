mod m20250806_183646_initial;
mod m20250806_191853_first;
mod m20250806_191954_create_views;
mod m20250806_202048_populate;
mod m20250918_132016_add_event_edition_maps_source;
mod m20250918_135135_add_event_edition_maps_thumbnail_source;
mod m20250918_215914_add_event_edition_maps_availability;
mod m20250918_222203_add_event_edition_maps_disability;
mod m20251002_100827_add_rm_mp_style_func;

use sea_orm_migration::prelude::*;

pub use sea_orm_migration::MigratorTrait;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20250806_183646_initial::Migration),
            Box::new(m20250806_191853_first::Migration),
            Box::new(m20250806_191954_create_views::Migration),
            Box::new(m20250806_202048_populate::Migration),
            Box::new(m20250918_132016_add_event_edition_maps_source::Migration),
            Box::new(m20250918_135135_add_event_edition_maps_thumbnail_source::Migration),
            Box::new(m20250918_215914_add_event_edition_maps_availability::Migration),
            Box::new(m20250918_222203_add_event_edition_maps_disability::Migration),
            Box::new(m20251002_100827_add_rm_mp_style_func::Migration),
        ]
    }
}
