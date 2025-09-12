mod m20250806_183646_initial;
mod m20250806_191853_first;
mod m20250806_191954_create_views;
mod m20250806_202048_populate;

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
        ]
    }
}
