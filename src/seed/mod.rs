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

const DEMO_PASSWORD: &str = "Demo1234!";

/// `(email, first name, last name, role code)`.
const DEMO_USERS: [(&str, &str, &str, &str); 4] = [
    ("admin@demo.local", "Ada", "Lovelace", "admin"),
    ("manager@demo.local", "Grace", "Hopper", "manager"),
    ("tech@demo.local", "Linus", "Torvalds", "technician"),
    ("viewer@demo.local", "Sam", "Carter", "viewer"),
];

/// `(asset_tag, product index, status code, department code, assignee, warranty offset, purchase offset, cost)`
const DEMO_ASSETS: [(&str, usize, &str, &str, Option<usize>, Option<i32>, i32, &str); 8] = [
    ("LAP-0001", 0, "deployed", "IT", Some(0), Some(-40), 900, "1450.00"),
    ("LAP-0002", 0, "deployed", "FIN", Some(1), Some(15), 620, "1450.00"),
    ("LAP-0003", 0, "ready_to_deploy", "IT", None, Some(45), 300, "1450.00"),
    ("LAP-0004", 0, "being_repaired", "COMM", None, Some(75), 430, "1450.00"),
    ("TLPH-0001", 1, "deployed", "COMM", Some(2), Some(200), 540, "280.00"),
    ("TLPH-0002", 1, "broken", "COMM", None, None, 540, "280.00"),
    ("HDST-0001", 2, "deployed", "IT", Some(3), Some(25), 210, "210.00"),
    ("HDST-0002", 2, "ready_to_deploy", "HR", None, Some(55), 120, "210.00"),
];

async fn seed_demo_users(
    conn: &mut PgConnection,
    org: Uuid,
    location_id: Uuid,
) -> Result<Vec<Uuid>, sqlx::Error> {
    let hash =
        crate::service::hash_password(DEMO_PASSWORD).map_err(|e| sqlx::Error::Protocol(e))?;
    let mut ids = Vec::with_capacity(DEMO_USERS.len());

    for (email, first_name, last_name, role_code) in DEMO_USERS.iter() {
        sqlx::query(
            "INSERT INTO users
                (organization_id, first_name, last_name, email, status, preferred_locale,
                 password_hash, location_id)
             VALUES ($1, $2, $3, $4, 'active', 'en', $5, $6)
             ON CONFLICT (organization_id, email) DO NOTHING",
        )
        .bind(org)
        .bind(first_name)
        .bind(last_name)
        .bind(email)
        .bind(&hash)
        .bind(location_id)
        .execute(&mut *conn)
        .await?;

        let id: Uuid =
            sqlx::query_scalar("SELECT id FROM users WHERE organization_id = $1 AND email = $2")
                .bind(org)
                .bind(email)
                .fetch_one(&mut *conn)
                .await?;
        ids.push(id);

        sqlx::query(
            "INSERT INTO user_roles (organization_id, user_id, role_id)
             SELECT $1, $2, id FROM roles WHERE code = $3
             ON CONFLICT DO NOTHING",
        )
        .bind(org)
        .bind(id)
        .bind(role_code)
        .execute(&mut *conn)
        .await?;
    }

    Ok(ids)
}

#[allow(clippy::too_many_arguments)]
async fn seed_demo_assets(
    conn: &mut PgConnection,
    org: Uuid,
    location_id: Uuid,
    laptop_product: Uuid,
    phone_product: Uuid,
    headset_product: Uuid,
    users: &[Uuid],
) -> Result<(), sqlx::Error> {
    let products = [laptop_product, phone_product, headset_product];
    let names = [
        "Dell Latitude 5440",
        "Cisco IP Phone 8841",
        "Jabra Evolve2 65",
    ];

    for (tag, product_idx, status_code, department_code, assignee, warranty_offset, purchase_offset, cost) in
        DEMO_ASSETS.iter()
    {
        let assigned_user_id = assignee.and_then(|idx| users.get(idx).copied());
        sqlx::query(
            "INSERT INTO assets
                (organization_id, asset_tag, product_id, status_id, location_id, department_id,
                 assigned_user_id, name, serial_number, warranty_date, purchase_date,
                 purchase_cost, currency)
             VALUES ($1, $2, $3,
                     (SELECT id FROM statuses WHERE organization_id = $1 AND code = $4),
                     $5,
                     (SELECT id FROM departments WHERE organization_id = $1 AND code = $6),
                     $7, $8, $9,
                     CASE WHEN $10::int IS NULL THEN NULL ELSE CURRENT_DATE + $10::int END,
                     CURRENT_DATE - $11::int, $12::numeric, 'USD')
             ON CONFLICT (organization_id, asset_tag) DO NOTHING",
        )
        .bind(org)
        .bind(tag)
        .bind(products[*product_idx])
        .bind(status_code)
        .bind(location_id)
        .bind(department_code)
        .bind(assigned_user_id)
        .bind(format!("{} ({})", names[*product_idx], tag))
        .bind(format!("SN-{tag}-{org}"))
        .bind(warranty_offset)
        .bind(purchase_offset)
        .bind(cost)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

async fn seed_demo_attachments(
    conn: &mut PgConnection,
    org: Uuid,
    users: &[Uuid],
) -> Result<(), sqlx::Error> {
    let asset_ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM assets WHERE organization_id = $1 ORDER BY asset_tag LIMIT 3",
    )
    .bind(org)
    .fetch_all(&mut *conn)
    .await?;

    let files = [
        ("invoice", "dell-invoice-2025.pdf", "application/pdf", 482_112i64),
        ("warranty", "warranty-certificate.pdf", "application/pdf", 128_004i64),
        ("damage_photo", "fan-damage.jpg", "image/jpeg", 1_204_992i64),
    ];

    for (index, (category, file_name, mime_type, size)) in files.iter().enumerate() {
        let Some(asset_id) = asset_ids.get(index) else {
            break;
        };
        let storage_key = format!("{org}/asset/{asset_id}/seed-{index}-{file_name}");
        sqlx::query(
            "INSERT INTO attachments
                (organization_id, attachable_type, attachable_id, category, file_name,
                 mime_type, size_bytes, storage_key, uploaded_by_user_id)
             VALUES ($1, 'asset', $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (storage_key) DO NOTHING",
        )
        .bind(org)
        .bind(asset_id)
        .bind(category)
        .bind(file_name)
        .bind(mime_type)
        .bind(size)
        .bind(&storage_key)
        .bind(users.get(index % users.len()).copied())
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// One open ticket on `LAP-0004` and one completed ticket on `LAP-0003`, created
/// through the real repository so asset statuses move and audits are emitted.
async fn seed_maintenance_workflow(
    pool: &PgPool,
    org: Uuid,
    actor: Uuid,
) -> Result<Vec<Uuid>, sqlx::Error> {
    use crate::modules::maintenances::dto::CreateMaintenanceRequest;
    use crate::modules::maintenances::repo as maintenance_repo;

    let existing: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM asset_maintenances WHERE organization_id = $1 ORDER BY created_at",
    )
    .bind(org)
    .fetch_all(pool)
    .await?;
    if !existing.is_empty() {
        return Ok(existing);
    }

    let assets: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT id, asset_tag FROM assets WHERE organization_id = $1
         AND asset_tag IN ('LAP-0003', 'LAP-0004') ORDER BY asset_tag",
    )
    .bind(org)
    .fetch_all(pool)
    .await?;
    let Some((repair_asset, _)) = assets.iter().find(|(_, tag)| tag == "LAP-0004") else {
        return Ok(Vec::new());
    };
    let Some((complete_asset, _)) = assets.iter().find(|(_, tag)| tag == "LAP-0003") else {
        return Ok(Vec::new());
    };

    let no_ip: Option<String> = None;
    let mut ids = Vec::new();

    let open_ticket = maintenance_repo::create_maintenance(
        pool,
        org,
        "en",
        actor,
        &no_ip,
        Some("seed"),
        &CreateMaintenanceRequest {
            asset_id: *repair_asset,
            title: "Laptop fan rattling and overheating".to_string(),
            description: Some(
                "User reports loud fan noise and thermal throttling under load.".to_string(),
            ),
            maintenance_type: Some("corrective".to_string()),
            priority: Some("high".to_string()),
            supplier_id: None,
            rma_reference: Some("RMA-2026-0042".to_string()),
            assigned_to_user_id: None,
            scheduled_at: None,
            cost: None,
            currency: None,
        },
    )
    .await
    .map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
    ids.push(open_ticket.id);

    let completed_ticket = maintenance_repo::create_maintenance(
        pool,
        org,
        "en",
        actor,
        &no_ip,
        Some("seed"),
        &CreateMaintenanceRequest {
            asset_id: *complete_asset,
            title: "Quarterly preventive inspection".to_string(),
            description: Some("Standard 90-day cleaning and thermal paste check.".to_string()),
            maintenance_type: Some("preventive".to_string()),
            priority: Some("medium".to_string()),
            supplier_id: None,
            rma_reference: None,
            assigned_to_user_id: None,
            scheduled_at: None,
            cost: None,
            currency: None,
        },
    )
    .await
    .map_err(|e| sqlx::Error::Protocol(e.to_string()))?;

    let completed = maintenance_repo::close_maintenance(
        pool,
        org,
        "en",
        actor,
        &no_ip,
        Some("seed"),
        completed_ticket.id,
        true,
        Some("Dust removed, thermal paste replaced, temperatures back to 62C under load."),
        Some(Decimal::new(8500, 2)),
        None,
    )
    .await
    .map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
    ids.push(completed.id);

    Ok(ids)
}

/// 55 backdated audit rows so the explorer timeline has content on day one.
async fn seed_audit_history(
    pool: &PgPool,
    org: Uuid,
    users: &[Uuid],
    maintenance_ids: &[Uuid],
) -> Result<usize, sqlx::Error> {
    let existing: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_logs
         WHERE organization_id = $1 AND metadata->>'source' = 'demo_history'",
    )
    .bind(org)
    .fetch_one(pool)
    .await?;
    if existing >= 50 {
        return Ok(0);
    }

    let asset_ids: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM assets WHERE organization_id = $1 ORDER BY asset_tag")
            .bind(org)
            .fetch_all(pool)
            .await?;

    let events = [
        "created",
        "updated",
        "assigned",
        "updated",
        "returned",
        "transferred",
        "maintenance_opened",
        "maintenance_closed",
    ];
    let entity_types = ["asset", "product", "license", "consumable", "accessory", "maintenance"];

    for index in 0..55usize {
        let event = events[index % events.len()];
        let entity_type = entity_types[(index * 5 + 2) % entity_types.len()];
        let entity_id = match entity_type {
            "asset" => asset_ids.get(index % asset_ids.len().max(1)).copied(),
            "maintenance" => maintenance_ids
                .get(index % maintenance_ids.len().max(1))
                .copied(),
            _ => None,
        };
        let actor = users.get(index % users.len()).copied();
        let hours_ago = (index as i64) * 3 + 2;

        let (old_values, new_values, diff) = match event {
            "created" => (
                None,
                Some(json!({ "status": "open", "created_at": "seed" })),
                None,
            ),
            "deleted" => (Some(json!({ "status": "archived" })), None, None),
            "updated" => {
                let delta = match entity_type {
                    "product" => json!({ "default_purchase_cost": { "old": 1450.00, "new": 1499.00 } }),
                    "license" => json!({ "total_seats": { "old": 10, "new": 12 } }),
                    "consumable" => json!({ "quantity_on_hand": { "old": 12, "new": 10 } }),
                    "accessory" => json!({ "available_quantity": { "old": 5, "new": 4 } }),
                    "maintenance" => json!({ "priority": { "old": "medium", "new": "high" } }),
                    _ => json!({ "status_id": { "old": "pending", "new": "deployed" } }),
                };
                (
                    Some(json!({ "field_count": 1 })),
                    Some(json!({ "field_count": 1 })),
                    Some(delta),
                )
            }
            "assigned" | "returned" | "transferred" => (
                Some(json!({ "assigned_user_id": null })),
                Some(json!({ "assigned_user_id": "demo" })),
                Some(json!({ "assigned_user_id": { "old": null, "new": "demo" } })),
            ),
            _ => (None, None, None),
        };

        let metadata = json!({
            "source": "demo_history",
            "note": format!("Seeded history row #{index}"),
        });
        let ip = format!("10.0.{}.{}", index % 40, 10 + index % 200);

        sqlx::query(
            "INSERT INTO audit_logs
                (organization_id, actor_user_id, action, entity_type, entity_id, metadata,
                 ip_address, actor_user_agent, old_values, new_values, diff, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11,
                     now() - make_interval(hours => $12::int))",
        )
        .bind(org)
        .bind(actor)
        .bind(event)
        .bind(entity_type)
        .bind(entity_id)
        .bind(metadata)
        .bind(ip)
        .bind("SeedAgent/1.0 (demo data)")
        .bind(old_values)
        .bind(new_values)
        .bind(diff)
        .bind(hours_ago as i32)
        .execute(pool)
        .await?;
    }

    Ok(55)
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

    let laptop_product = upsert_product(
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

    let phone_product = upsert_product(
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

    let headset_product = upsert_product(
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

    // Demo dataset: users, assets, attachment metadata. Maintenance tickets
    // and the audit history are created after the commit because they reuse
    // the domain repositories (and therefore exercise the real transitions).
    let hq = location_ids
        .iter()
        .find(|(code, _)| **code == "HQ")
        .map(|(_, id)| *id)
        .ok_or("Missing HQ location")?;
    let demo_users = seed_demo_users(&mut tx, org, hq)
        .await
        .map_err(|e| e.to_string())?;
    seed_demo_assets(
        &mut tx,
        org,
        hq,
        laptop_product,
        phone_product,
        headset_product,
        &demo_users,
    )
    .await
    .map_err(|e| e.to_string())?;
    seed_demo_attachments(&mut tx, org, &demo_users)
        .await
        .map_err(|e| e.to_string())?;

    audit::insert_tx(
        &mut tx,
        audit::Entry {
            organization_id: Some(org),
            actor_user_id: None,
            action: "seeded",
            entity_type: "organization",
            entity_id: Some(org),
            metadata: json!({ "source": "default_seed" }),
            ip_address: None,
        },
    )
    .await
    .map_err(|e| e.to_string())?;

    tx.commit().await.map_err(|e| e.to_string())?;

    let maintenance_ids = seed_maintenance_workflow(pool, org, demo_users[0])
        .await
        .map_err(|e| e.to_string())?;
    let history_rows = seed_audit_history(pool, org, &demo_users, &maintenance_ids)
        .await
        .map_err(|e| e.to_string())?;
    println!("demo_audit_history_rows={history_rows}");

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
        "users",
        "assets",
        "asset_maintenances",
        "attachments",
        "audit_logs",
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
