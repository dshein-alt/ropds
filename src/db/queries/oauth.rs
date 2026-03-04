use crate::db::DbPool;
use crate::db::models::OAuthIdentity;

/// Insert a new oauth_identities row with status = 'pending'.
pub async fn create_identity(
    pool: &DbPool,
    user_id: i64,
    provider: &str,
    provider_uid: &str,
    email: Option<&str>,
    display_name: Option<&str>,
) -> Result<(), sqlx::Error> {
    let now = chrono::Utc::now().to_rfc3339();
    let sql = pool.sql(
        "INSERT INTO oauth_identities \
         (user_id, provider, provider_uid, email, display_name, status, created_at) \
         VALUES (?, ?, ?, ?, ?, 'pending', ?)",
    );
    sqlx::query(&sql)
        .bind(user_id)
        .bind(provider)
        .bind(provider_uid)
        .bind(email)
        .bind(display_name)
        .bind(&now)
        .execute(pool.inner())
        .await?;
    Ok(())
}

/// Find an identity by provider + provider_uid.
pub async fn find_by_provider(
    pool: &DbPool,
    provider: &str,
    provider_uid: &str,
) -> Result<Option<OAuthIdentity>, sqlx::Error> {
    let sql = pool.sql(
        "SELECT id, user_id, provider, provider_uid, email, display_name, \
                status, rejected_at, created_at \
         FROM oauth_identities WHERE provider = ? AND provider_uid = ?",
    );
    sqlx::query_as(&sql)
        .bind(provider)
        .bind(provider_uid)
        .fetch_optional(pool.inner())
        .await
}

/// Find identity by internal id.
pub async fn get_by_id(
    pool: &DbPool,
    identity_id: i64,
) -> Result<Option<OAuthIdentity>, sqlx::Error> {
    let sql = pool.sql(
        "SELECT id, user_id, provider, provider_uid, email, display_name, \
                status, rejected_at, created_at \
         FROM oauth_identities WHERE id = ?",
    );
    sqlx::query_as(&sql)
        .bind(identity_id)
        .fetch_optional(pool.inner())
        .await
}

/// Update identity status and optionally set/clear rejected_at.
pub async fn update_status(
    pool: &DbPool,
    user_id: i64,
    provider: &str,
    provider_uid: &str,
    status: &str,
    rejected_at: Option<&str>,
) -> Result<(), sqlx::Error> {
    let sql = pool.sql(
        "UPDATE oauth_identities SET status = ?, rejected_at = ? \
         WHERE user_id = ? AND provider = ? AND provider_uid = ?",
    );
    sqlx::query(&sql)
        .bind(status)
        .bind(rejected_at.filter(|s| !s.is_empty()))
        .bind(user_id)
        .bind(provider)
        .bind(provider_uid)
        .execute(pool.inner())
        .await?;
    Ok(())
}

/// Update status by identity id (used by admin approve/reject/ban/reinstate).
pub async fn update_status_by_id(
    pool: &DbPool,
    identity_id: i64,
    status: &str,
    rejected_at: Option<&str>,
) -> Result<(), sqlx::Error> {
    let sql = pool.sql("UPDATE oauth_identities SET status = ?, rejected_at = ? WHERE id = ?");
    sqlx::query(&sql)
        .bind(status)
        .bind(rejected_at.filter(|s| !s.is_empty()))
        .bind(identity_id)
        .execute(pool.inner())
        .await?;
    Ok(())
}

/// Reassign identity to another local user.
pub async fn reassign_user_by_id(
    pool: &DbPool,
    identity_id: i64,
    target_user_id: i64,
) -> Result<(), sqlx::Error> {
    let sql = pool.sql("UPDATE oauth_identities SET user_id = ? WHERE id = ?");
    sqlx::query(&sql)
        .bind(target_user_id)
        .bind(identity_id)
        .execute(pool.inner())
        .await?;
    Ok(())
}

/// List all identities with a given status, newest first.
pub async fn list_by_status(
    pool: &DbPool,
    status: &str,
) -> Result<Vec<OAuthIdentity>, sqlx::Error> {
    let sql = pool.sql(
        "SELECT id, user_id, provider, provider_uid, email, display_name, \
                status, rejected_at, created_at \
         FROM oauth_identities WHERE status = ? ORDER BY created_at DESC",
    );
    sqlx::query_as(&sql)
        .bind(status)
        .fetch_all(pool.inner())
        .await
}

/// Count pending identities (for admin nav badge).
pub async fn count_pending(pool: &DbPool) -> Result<i64, sqlx::Error> {
    let sql = pool.sql("SELECT COUNT(*) FROM oauth_identities WHERE status = 'pending'");
    let row: (i64,) = sqlx::query_as(&sql).fetch_one(pool.inner()).await?;
    Ok(row.0)
}

/// Count identities linked to a local user.
pub async fn count_for_user(pool: &DbPool, user_id: i64) -> Result<i64, sqlx::Error> {
    let sql = pool.sql("SELECT COUNT(*) FROM oauth_identities WHERE user_id = ?");
    let row: (i64,) = sqlx::query_as(&sql)
        .bind(user_id)
        .fetch_one(pool.inner())
        .await?;
    Ok(row.0)
}

/// List all identities for a given user (to check if any are active).
pub async fn list_for_user(pool: &DbPool, user_id: i64) -> Result<Vec<OAuthIdentity>, sqlx::Error> {
    let sql = pool.sql(
        "SELECT id, user_id, provider, provider_uid, email, display_name, \
                status, rejected_at, created_at \
         FROM oauth_identities WHERE user_id = ?",
    );
    sqlx::query_as(&sql)
        .bind(user_id)
        .fetch_all(pool.inner())
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;
    use crate::db::queries::users::create_oauth_user;

    async fn make_user(pool: &crate::db::DbPool, name: &str) -> i64 {
        create_oauth_user(pool, name, "", 0, name).await.unwrap()
    }

    #[tokio::test]
    async fn test_create_and_find_identity() {
        let pool = create_test_pool().await;
        let uid = make_user(&pool, "guser").await;
        create_identity(
            &pool,
            uid,
            "google",
            "g-123",
            Some("a@b.com"),
            Some("Alice"),
        )
        .await
        .unwrap();
        let identity = find_by_provider(&pool, "google", "g-123")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(identity.user_id, uid);
        assert_eq!(identity.status, "pending");
    }

    #[tokio::test]
    async fn test_update_status() {
        let pool = create_test_pool().await;
        let uid = make_user(&pool, "guser2").await;
        create_identity(&pool, uid, "google", "g-456", None, None)
            .await
            .unwrap();
        update_status(&pool, uid, "google", "g-456", "active", None)
            .await
            .unwrap();
        let identity = find_by_provider(&pool, "google", "g-456")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(identity.status, "active");
    }

    #[tokio::test]
    async fn test_list_by_status() {
        let pool = create_test_pool().await;
        let uid = make_user(&pool, "guser3").await;
        create_identity(&pool, uid, "google", "g-789", None, None)
            .await
            .unwrap();
        let pending = list_by_status(&pool, "pending").await.unwrap();
        assert!(!pending.is_empty());
    }

    #[tokio::test]
    async fn test_get_by_id_reassign_and_count_for_user() {
        let pool = create_test_pool().await;
        let source = make_user(&pool, "source").await;
        let target = make_user(&pool, "target").await;

        create_identity(&pool, source, "google", "g-999", None, Some("Source User"))
            .await
            .unwrap();

        let ident = find_by_provider(&pool, "google", "g-999")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(count_for_user(&pool, source).await.unwrap(), 1);
        assert_eq!(count_for_user(&pool, target).await.unwrap(), 0);

        reassign_user_by_id(&pool, ident.id, target).await.unwrap();

        let moved = get_by_id(&pool, ident.id).await.unwrap().unwrap();
        assert_eq!(moved.user_id, target);
        assert_eq!(count_for_user(&pool, source).await.unwrap(), 0);
        assert_eq!(count_for_user(&pool, target).await.unwrap(), 1);
    }
}
