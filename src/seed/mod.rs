// Default lookup + catalog seeder for one organization.
//
//   cargo run -- seed --org-slug acme
//
// Idempotent: rows are matched by code (or English name for the lookup tables
// that have no code column), then translations are upserted. Re-running only
// refreshes names and never duplicates rows.

use log::info;
use rust_decimal::Decimal;
use serde_json::json;
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::config::database::{Database, DatabaseTrait};
use crate::repository::audit;

struct Names {
    en: &'static str,
    fr: &'static str,
    ar: &'static str,
}

const DEPARTMENTS: [(&str, Names); 4] = [
    (
        "COMM",
        Names {
            en: "Communication",
            fr: "Communication",
            ar: "الاتصالات",
        },
    ),
    (
        "IT",
        Names {
            en: "IT",
            fr: "Informatique",
            ar: "تقنية المعلومات",
        },
    ),
    (
        "FIN",
        Names {
            en: "Finance",
            fr: "Finance",
            ar: "المالية",
        },
    ),
    (
        "HR",
        Names {
            en: "Human Resources",
            fr: "Ressources Humaines",
            ar: "الموارد البشرية",
        },
    ),
];

const CATEGORIES: [(&str, &str, Names); 4] = [
    (
        "ACC",
        "accessory",
        Names {
            en: "Accessory",
            fr: "Accessoire",
            ar: "ملحق",
        },
    ),
    (
        "AST",
        "asset",
        Names {
            en: "Asset",
            fr: "Actif",
            ar: "أصل",
        },
    ),
    (
        "CNS",
        "consumable",
        Names {
            en: "Consumable",
            fr: "Consommable",
            ar: "مستهلك",
        },
    ),
    (
        "CMP",
        "component",
        Names {
            en: "Component",
            fr: "Composant",
            ar: "مكون",
        },
    ),
];

const MANUFACTURERS: [Names; 4] = [
    Names {
        en: "Dell",
        fr: "Dell",
        ar: "ديل",
    },
    Names {
        en: "HP",
        fr: "HP",
        ar: "إتش بي",
    },
    Names {
        en: "Cisco",
        fr: "Cisco",
        ar: "سيسكو",
    },
    Names {
        en: "Jabra",
        fr: "Jabra",
        ar: "جابرا",
    },
];

// (code, status_type, color_code, show_in_nav, names)
const STATUSES: [(&str, &str, &str, bool, Names); 7] = [
    (
        "archived",
        "archived",
        "#6B7280",
        false,
        Names {
            en: "Archived",
            fr: "Archivé",
            ar: "مؤرشف",
        },
    ),
    (
        "being_repaired",
        "pending",
        "#F59E0B",
        false,
        Names {
            en: "Being Repaired",
            fr: "En réparation",
            ar: "قيد الإصلاح",
        },
    ),
    (
        "broken",
        "undeployable",
        "#EF4444",
        false,
        Names {
            en: "Broken",
            fr: "En panne",
            ar: "معطل",
        },
    ),
    (
        "deployed",
        "deployed",
        "#10B981",
        true,
        Names {
            en: "Deployed",
            fr: "Déployé",
            ar: "موزع",
        },
    ),
    (
        "lost_or_stolen",
        "undeployable",
        "#7C3AED",
        false,
        Names {
            en: "Lost or Stolen",
            fr: "Perdu ou volé",
            ar: "مفقود أو مسروق",
        },
    ),
    (
        "pending",
        "pending",
        "#F97316",
        false,
        Names {
            en: "Pending",
            fr: "En attente",
            ar: "معلق",
        },
    ),
    (
        "ready_to_deploy",
        "deployable",
        "#3B82F6",
        true,
        Names {
            en: "Ready to Deploy",
            fr: "Prêt à déployer",
            ar: "جاهز للتوزيع",
        },
    ),
];

// (code, location_type, parent code, city, names)
const LOCATIONS: [(&str, &str, Option<&str>, &str, Names); 4] = [
    (
        "HQ",
        "site",
        None,
        "Tunis",
        Names {
            en: "Headquarters",
            fr: "Siège Social",
            ar: "المقر الرئيسي",
        },
    ),
    (
        "HQ-WH",
        "building",
        Some("HQ"),
        "Tunis",
        Names {
            en: "Main Warehouse",
            fr: "Entrepôt Principal",
            ar: "المستودع الرئيسي",
        },
    ),
    (
        "SITE-SFAX",
        "site",
        None,
        "Sfax",
        Names {
            en: "Sfax Office",
            fr: "Bureau de Sfax",
            ar: "مكتب صفاقس",
        },
    ),
    (
        "SITE-SOUSSE",
        "site",
        None,
        "Sousse",
        Names {
            en: "Sousse Office",
            fr: "Bureau de Sousse",
            ar: "مكتب سوسة",
        },
    ),
];

const SUPPLIERS: [(&str, Names); 2] = [
    (
        "Karim Ben Ali",
        Names {
            en: "Tech Distribution SARL",
            fr: "Tech Distribution SARL",
            ar: "تك للتوزيع",
        },
    ),
    (
        "Sonia Trabelsi",
        Names {
            en: "Global Office Supply",
            fr: "Fournitures de Bureau Globales",
            ar: "الإمدادات المكتبية العالمية",
        },
    ),
];

const DEPRECIATION_MODELS: [(i32, Names); 2] = [
    (
        36,
        Names {
            en: "Straight line 36 months",
            fr: "Amortissement linéaire 36 mois",
            ar: "إهلاك خطي 36 شهرًا",
        },
    ),
    (
        60,
        Names {
            en: "Straight line 60 months",
            fr: "Amortissement linéaire 60 mois",
            ar: "إهلاك خطي 60 شهرًا",
        },
    ),
];

/// Insert (or find) a localized lookup row, then upsert its three translations.
///
/// `extra_columns` / `extra_values` are static SQL fragments owned by this
/// module, never user input.
async fn upsert_localized(
    conn: &mut PgConnection,
    table: &str,
    translations_table: &str,
    fk: &str,
    org: Uuid,
    code: Option<&str>,
    names: &Names,
    extra_columns: &str,
    extra_values: &str,
) -> Result<Uuid, sqlx::Error> {
    let id = match code {
        Some(code) => {
            let sql = format!(
                "INSERT INTO {table} (organization_id, code{extra_columns})
                 VALUES ($1, $2{extra_values})
                 ON CONFLICT (organization_id, code) DO UPDATE SET updated_at = now()
                 RETURNING id"
            );
            sqlx::query_scalar::<_, Uuid>(&sql)
                .bind(org)
                .bind(code)
                .fetch_optional(&mut *conn)
                .await?
        }
        None => {
            // No natural key: match the existing row by its English name.
            let find = format!(
                "SELECT t.{fk} FROM {translations_table} t
                 JOIN {table} e ON e.id = t.{fk}
                 WHERE e.organization_id = $1 AND t.locale = 'en' AND t.name = $2
                 LIMIT 1"
            );
            let existing = sqlx::query_scalar::<_, Uuid>(&find)
                .bind(org)
                .bind(names.en)
                .fetch_optional(&mut *conn)
                .await?;
            match existing {
                Some(id) => Some(id),
                None => {
                    let insert = format!(
                        "INSERT INTO {table} (organization_id{extra_columns})
                         VALUES ($1{extra_values}) RETURNING id"
                    );
                    sqlx::query_scalar::<_, Uuid>(&insert)
                        .bind(org)
                        .fetch_optional(&mut *conn)
                        .await?
                }
            }
        }
    };
    let id = id.ok_or(sqlx::Error::RowNotFound)?;

    let sql = format!(
        "INSERT INTO {translations_table} ({fk}, locale, name) VALUES ($1, $2, $3)
         ON CONFLICT ({fk}, locale) DO UPDATE SET name = EXCLUDED.name"
    );
    for (locale, name) in [("en", names.en), ("fr", names.fr), ("ar", names.ar)] {
        sqlx::query(&sql)
            .bind(id)
            .bind(locale)
            .bind(name)
            .execute(&mut *conn)
            .await?;
    }
    Ok(id)
}

async fn upsert_product(
    conn: &mut PgConnection,
    org: Uuid,
    code: &str,
    names: &Names,
    category_id: Uuid,
    manufacturer_id: Option<Uuid>,
    model_code: &str,
    cost: &str,
    specs: serde_json::Value,
) -> Result<Uuid, sqlx::Error> {
    // `cost` is a static numeric literal from this module, not user input.
    let sql = format!(
        "INSERT INTO products (organization_id, code, category_id, manufacturer_id, model_code,
                               eol_type, default_purchase_cost, default_currency, specs_json)
         VALUES ($1, $2, $3, $4, $5, 'months_from_purchase', {cost}, 'USD', $6)
         ON CONFLICT (organization_id, code) DO UPDATE SET updated_at = now()
         RETURNING id"
    );
    let id = sqlx::query_scalar::<_, Uuid>(&sql)
        .bind(org)
        .bind(code)
        .bind(category_id)
        .bind(manufacturer_id)
        .bind(model_code)
        .bind(specs)
        .fetch_one(&mut *conn)
        .await?;

    let names_sql = "INSERT INTO product_translations (product_id, locale, name)
                     VALUES ($1, $2, $3)
                     ON CONFLICT (product_id, locale) DO UPDATE SET name = EXCLUDED.name";
    for (locale, name) in [("en", names.en), ("fr", names.fr), ("ar", names.ar)] {
        sqlx::query(names_sql)
            .bind(id)
            .bind(locale)
            .bind(name)
            .execute(&mut *conn)
            .await?;
    }
    Ok(id)
}

// ------------------------------------------- accessories / consumables ------
// (names, total, min, cost in cents)
const ACCESSORIES: [(&Names, i32, i32, i64); 4] = [
    (
        &Names {
            en: "USB-C Charger 65W",
            fr: "Chargeur USB-C 65W",
            ar: "شاحن USB-C 65 واط",
        },
        40,
        10,
        4900,
    ),
    (
        &Names {
            en: "Laptop Sleeve 14\"",
            fr: "Housse d'ordinateur 14 pouces",
            ar: "حقيبة حاسوب 14 بوصة",
        },
        25,
        5,
        2500,
    ),
    (
        &Names {
            en: "Logitech Webcam C920",
            fr: "Webcam Logitech C920",
            ar: "كاميرا لوجيتك C920",
        },
        15,
        3,
        7900,
    ),
    (
        &Names {
            en: "USB-C Docking Station",
            fr: "Station d'accueil USB-C",
            ar: "محطة توصيل USB-C",
        },
        8,
        4,
        18000,
    ),
];

// (names, opening stock, min stock alert, cost in cents)
const CONSUMABLES: [(&Names, i32, i32, i64); 4] = [
    (
        &Names {
            en: "HP 26A Toner Cartridge",
            fr: "Cartouche de toner HP 26A",
            ar: "خرطوشة حبر HP 26A",
        },
        3,
        5,
        8900,
    ),
    (
        &Names {
            en: "A4 Paper Ream",
            fr: "Ramette de papier A4",
            ar: "رزمة ورق A4",
        },
        40,
        10,
        450,
    ),
    (
        &Names {
            en: "AA Batteries (4-pack)",
            fr: "Piles AA (lot de 4)",
            ar: "بطاريات AA (4 قطع)",
        },
        2,
        6,
        600,
    ),
    (
        &Names {
            en: "Whiteboard Markers (4-pack)",
            fr: "Marqueurs tableau blanc (lot de 4)",
            ar: "أقلام سبورة بيضاء (4 قطع)",
        },
        12,
        4,
        900,
    ),
];

// (names, total seats, days until expiry, cost in cents)
const LICENSES: [(&Names, i32, i64, i64); 5] = [
    (
        &Names {
            en: "Microsoft 365 E3",
            fr: "Microsoft 365 E3",
            ar: "مايكروسوفت 365 E3",
        },
        25,
        300,
        420000,
    ),
    (
        &Names {
            en: "Adobe Acrobat Pro",
            fr: "Adobe Acrobat Pro",
            ar: "أدوبي أكروبات برو",
        },
        10,
        25,
        180000,
    ),
    (
        &Names {
            en: "Slack Business+",
            fr: "Slack Business+",
            ar: "سلاك بزنس+",
        },
        50,
        75,
        600000,
    ),
    (
        &Names {
            en: "Zoom Workplace Pro",
            fr: "Zoom Workplace Pro",
            ar: "زووم ورك بليس برو",
        },
        30,
        55,
        360000,
    ),
    (
        &Names {
            en: "JetBrains All Products Pack",
            fr: "Pack JetBrains All Products",
            ar: "حزمة جيتبريونز",
        },
        8,
        88,
        240000,
    ),
];

async fn find_localized_id(
    conn: &mut PgConnection,
    table: &str,
    translations_table: &str,
    fk: &str,
    org: Uuid,
    names: &Names,
) -> sqlx::Result<Option<Uuid>> {
    let sql = format!(
        "SELECT t.{fk} FROM {translations_table} t
         JOIN {table} e ON e.id = t.{fk}
         WHERE e.organization_id = $1 AND t.locale = 'en' AND t.name = $2
         LIMIT 1"
    );
    sqlx::query_scalar::<_, Uuid>(&sql)
        .bind(org)
        .bind(names.en)
        .fetch_optional(&mut *conn)
        .await
}

async fn put_translations(
    conn: &mut PgConnection,
    translations_table: &str,
    fk: &str,
    id: Uuid,
    names: &Names,
) -> sqlx::Result<()> {
    let sql = format!(
        "INSERT INTO {translations_table} ({fk}, locale, name) VALUES ($1, $2, $3)
         ON CONFLICT ({fk}, locale) DO UPDATE SET name = EXCLUDED.name"
    );
    for (locale, name) in [("en", names.en), ("fr", names.fr), ("ar", names.ar)] {
        sqlx::query(&sql)
            .bind(id)
            .bind(locale)
            .bind(name)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

async fn upsert_accessory(
    conn: &mut PgConnection,
    org: Uuid,
    names: &Names,
    total: i32,
    min: i32,
    cents: i64,
) -> sqlx::Result<Uuid> {
    let cost = Decimal::new(cents, 2);
    let existing = find_localized_id(
        conn,
        "accessories",
        "accessory_translations",
        "accessory_id",
        org,
        names,
    )
    .await?;
    let id = match existing {
        Some(id) => {
            // Never overwrite live stock quantities on a re-run.
            sqlx::query(
                "UPDATE accessories SET min_quantity = $3, purchase_cost = $4, updated_at = now()
                 WHERE id = $1 AND organization_id = $2",
            )
            .bind(id)
            .bind(org)
            .bind(min)
            .bind(cost)
            .execute(&mut *conn)
            .await?;
            id
        }
        None => sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO accessories
                (organization_id, total_quantity, available_quantity, min_quantity, purchase_cost, currency)
             VALUES ($1, $2, $2, $3, $4, 'USD') RETURNING id",
        )
        .bind(org)
        .bind(total)
        .bind(min)
        .bind(cost)
        .fetch_one(&mut *conn)
        .await?,
    };
    put_translations(conn, "accessory_translations", "accessory_id", id, names).await?;
    Ok(id)
}

async fn upsert_consumable(
    conn: &mut PgConnection,
    org: Uuid,
    names: &Names,
    opening_stock: i32,
    min_alert: i32,
    cents: i64,
) -> sqlx::Result<Uuid> {
    let cost = Decimal::new(cents, 2);
    let existing = find_localized_id(
        conn,
        "consumables",
        "consumable_translations",
        "consumable_id",
        org,
        names,
    )
    .await?;
    let id = match existing {
        Some(id) => {
            sqlx::query(
                "UPDATE consumables SET min_stock_alert = $3, purchase_cost = $4, updated_at = now()
                 WHERE id = $1 AND organization_id = $2",
            )
            .bind(id)
            .bind(org)
            .bind(min_alert)
            .bind(cost)
            .execute(&mut *conn)
            .await?;
            id
        }
        None => {
            let id = sqlx::query_scalar::<_, Uuid>(
                "INSERT INTO consumables
                    (organization_id, quantity_on_hand, min_stock_alert, purchase_cost, currency)
                 VALUES ($1, $2, $3, $4, 'USD') RETURNING id",
            )
            .bind(org)
            .bind(opening_stock)
            .bind(min_alert)
            .bind(cost)
            .fetch_one(&mut *conn)
            .await?;
            if opening_stock > 0 {
                sqlx::query(
                    "INSERT INTO consumable_transactions
                        (organization_id, consumable_id, transaction_type, quantity, notes)
                     VALUES ($1, $2, 'restock', $3, 'Opening stock')",
                )
                .bind(org)
                .bind(id)
                .bind(opening_stock)
                .execute(&mut *conn)
                .await?;
            }
            id
        }
    };
    put_translations(conn, "consumable_translations", "consumable_id", id, names).await?;
    Ok(id)
}

async fn upsert_license(
    conn: &mut PgConnection,
    org: Uuid,
    names: &Names,
    total_seats: i32,
    expires_in_days: i64,
    cents: i64,
) -> sqlx::Result<Uuid> {
    let cost = Decimal::new(cents, 2);
    let existing = find_localized_id(
        conn,
        "licenses",
        "license_translations",
        "license_id",
        org,
        names,
    )
    .await?;
    let id = match existing {
        Some(id) => {
            sqlx::query(
                "UPDATE licenses
                 SET total_seats = $3, expiration_date = CURRENT_DATE + $4, purchase_cost = $5,
                     updated_at = now()
                 WHERE id = $1 AND organization_id = $2",
            )
            .bind(id)
            .bind(org)
            .bind(total_seats)
            .bind(expires_in_days as i32)
            .bind(cost)
            .execute(&mut *conn)
            .await?;
            id
        }
        None => {
            sqlx::query_scalar::<_, Uuid>(
                "INSERT INTO licenses
                (organization_id, total_seats, expiration_date, purchase_cost, currency)
             VALUES ($1, $2, CURRENT_DATE + $3, $4, 'USD') RETURNING id",
            )
            .bind(org)
            .bind(total_seats)
            .bind(expires_in_days as i32)
            .bind(cost)
            .fetch_one(&mut *conn)
            .await?
        }
    };
    put_translations(conn, "license_translations", "license_id", id, names).await?;
    Ok(id)
}

async fn organization_id(pool: &PgPool, slug: &str) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar::<_, Uuid>("SELECT id FROM organizations WHERE slug = $1")
        .bind(slug)
        .fetch_one(pool)
        .await
}

/// Seed every default lookup, plus a small product catalog for demo assets.
pub async fn run(db: &Database, args: Vec<String>) -> Result<(), String> {
    let mut org_slug = None;
    let mut iter = args.into_iter();
    while let Some(flag) = iter.next() {
        match flag.as_str() {
            "--org-slug" => org_slug = iter.next(),
            other => return Err(format!("Unexpected argument: {other}")),
        }
    }
    let org_slug = org_slug
        .map(|slug| slug.to_lowercase())
        .filter(|slug| !slug.trim().is_empty())
        .ok_or("--org-slug is required")?;

    let pool = db.get_pool();
    let org = organization_id(pool, &org_slug)
        .await
        .map_err(|e| format!("Organization '{org_slug}' not found: {e}"))?;

    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;

    for (code, names) in DEPARTMENTS.iter() {
        upsert_localized(
            &mut tx,
            "departments",
            "department_translations",
            "department_id",
            org,
            Some(code),
            names,
            "",
            "",
        )
        .await
        .map_err(|e| e.to_string())?;
    }

    for (code, category_type, names) in CATEGORIES.iter() {
        upsert_localized(
            &mut tx,
            "categories",
            "category_translations",
            "category_id",
            org,
            Some(code),
            names,
            ", category_type",
            &format!(", '{category_type}'"),
        )
        .await
        .map_err(|e| e.to_string())?;
    }

    let mut manufacturer_ids = Vec::new();
    for names in MANUFACTURERS.iter() {
        manufacturer_ids.push(
            upsert_localized(
                &mut tx,
                "manufacturers",
                "manufacturer_translations",
                "manufacturer_id",
                org,
                None,
                names,
                "",
                "",
            )
            .await
            .map_err(|e| e.to_string())?,
        );
    }

    for (code, status_type, color, show_in_nav, names) in STATUSES.iter() {
        upsert_localized(
            &mut tx,
            "statuses",
            "status_translations",
            "status_id",
            org,
            Some(code),
            names,
            ", status_type, color_code, show_in_nav",
            &format!(", '{status_type}', '{color}', {show_in_nav}"),
        )
        .await
        .map_err(|e| e.to_string())?;
    }

    let mut location_ids = Vec::new();
    for (code, location_type, _, city, names) in LOCATIONS.iter() {
        let id = upsert_localized(
            &mut tx,
            "locations",
            "location_translations",
            "location_id",
            org,
            Some(code),
            names,
            ", location_type, city, country_code",
            &format!(", '{location_type}', '{city}', 'TN'"),
        )
        .await
        .map_err(|e| e.to_string())?;
        location_ids.push((code, id));
    }
    // Second pass: hierarchy needs the parent ids.
    for (code, _, parent, _, _) in LOCATIONS.iter() {
        if let Some(parent_code) = parent {
            let parent_id = location_ids
                .iter()
                .find(|(candidate, _)| *candidate == parent_code)
                .map(|(_, id)| *id)
                .ok_or_else(|| format!("Missing parent location {parent_code}"))?;
            sqlx::query("UPDATE locations SET parent_id = $1, updated_at = now() WHERE organization_id = $2 AND code = $3")
                .bind(parent_id)
                .bind(org)
                .bind(code)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
        }
    }

    for (contact_name, names) in SUPPLIERS.iter() {
        upsert_localized(
            &mut tx,
            "suppliers",
            "supplier_translations",
            "supplier_id",
            org,
            None,
            names,
            ", contact_name",
            &format!(", '{contact_name}'"),
        )
        .await
        .map_err(|e| e.to_string())?;
    }

    for (months, names) in DEPRECIATION_MODELS.iter() {
        upsert_localized(
            &mut tx,
            "depreciation_models",
            "depreciation_model_translations",
            "depreciation_model_id",
            org,
            None,
            names,
            ", months",
            &format!(", {months}"),
        )
        .await
        .map_err(|e| e.to_string())?;
    }

    let asset_category = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM categories WHERE organization_id = $1 AND code = 'AST'",
    )
    .bind(org)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;
    let accessory_category = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM categories WHERE organization_id = $1 AND code = 'ACC'",
    )
    .bind(org)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    let (dell, cisco, jabra) = (
        Some(manufacturer_ids[0]),
        Some(manufacturer_ids[2]),
        Some(manufacturer_ids[3]),
    );

    upsert_product(
        &mut tx,
        org,
        "LAP",
        &Names {
            en: "Dell Latitude 5440",
            fr: "Dell Latitude 5440",
            ar: "ديل لاتيتود 5440",
        },
        asset_category,
        dell,
        "Latitude 5440",
        "1450.00",
        json!({ "cpu": "Intel Core i5-1335U", "ram": "16GB", "storage": "512GB SSD" }),
    )
    .await
    .map_err(|e| e.to_string())?;

    upsert_product(
        &mut tx,
        org,
        "TLPH",
        &Names {
            en: "Cisco IP Phone 8841",
            fr: "Cisco IP Phone 8841",
            ar: "هاتف سيسكو IP 8841",
        },
        asset_category,
        cisco,
        "CP-8841-K9",
        "280.00",
        json!({ "protocol": "SIP", "lines": 5 }),
    )
    .await
    .map_err(|e| e.to_string())?;

    upsert_product(
        &mut tx,
        org,
        "HDST",
        &Names {
            en: "Jabra Evolve2 65",
            fr: "Jabra Evolve2 65",
            ar: "جابرا إيفولف 2 65",
        },
        accessory_category,
        jabra,
        "Evolve2 65",
        "210.00",
        json!({ "connectivity": "Bluetooth 5.0", "battery": "37h" }),
    )
    .await
    .map_err(|e| e.to_string())?;

    for (names, total, min, cents) in ACCESSORIES.iter() {
        upsert_accessory(&mut tx, org, names, *total, *min, *cents)
            .await
            .map_err(|e| e.to_string())?;
    }
    for (names, opening_stock, min_alert, cents) in CONSUMABLES.iter() {
        upsert_consumable(&mut tx, org, names, *opening_stock, *min_alert, *cents)
            .await
            .map_err(|e| e.to_string())?;
    }
    for (names, total_seats, expires_in_days, cents) in LICENSES.iter() {
        upsert_license(&mut tx, org, names, *total_seats, *expires_in_days, *cents)
            .await
            .map_err(|e| e.to_string())?;
    }

    audit::insert_tx(
        &mut tx,
        audit::Entry {
            organization_id: Some(org),
            actor_user_id: None,
            action: "lookups.seeded",
            entity_type: "organization",
            entity_id: Some(org),
            metadata: json!({ "source": "default_seed" }),
            ip_address: None,
        },
    )
    .await
    .map_err(|e| e.to_string())?;

    tx.commit().await.map_err(|e| e.to_string())?;

    for table in [
        "departments",
        "categories",
        "manufacturers",
        "statuses",
        "locations",
        "suppliers",
        "depreciation_models",
        "products",
        "accessories",
        "consumables",
        "licenses",
    ] {
        let count = sqlx::query_scalar::<_, i64>(&format!(
            "SELECT count(*) FROM {table} WHERE organization_id = $1"
        ))
        .bind(org)
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?;
        println!("{table}={count}");
    }

    info!("Seeded default lookups for organization {org_slug}");
    Ok(())
}
