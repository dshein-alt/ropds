use sqlx::FromRow;

use crate::db::DbPool;
use crate::db::models::User;

/// User data safe for template rendering (no password_hash).
#[derive(Debug, Clone, FromRow, serde::Serialize)]
pub struct UserView {
    pub id: i64,
    pub username: String,
    pub is_superuser: i32,
    pub created_at: String,
    pub last_login: String,
}

/// Get all users for admin panel listing (excludes password_hash).
pub async fn get_all_views(pool: &DbPool) -> Result<Vec<UserView>, sqlx::Error> {
    let users: Vec<UserView> = sqlx::query_as(
        "SELECT id, username, is_superuser, created_at, last_login FROM users ORDER BY id"
    )
    .fetch_all(pool)
    .await?;
    Ok(users)
}

/// Get a single user by ID.
pub async fn get_by_id(pool: &DbPool, user_id: i64) -> Result<Option<User>, sqlx::Error> {
    let user: Option<User> = sqlx::query_as(
        "SELECT id, username, password_hash, is_superuser, created_at, last_login FROM users WHERE id = ?"
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    Ok(user)
}

/// Get `is_superuser` flag for a given user ID.
pub async fn is_superuser(pool: &DbPool, user_id: i64) -> Result<bool, sqlx::Error> {
    let row: Option<(i32,)> = sqlx::query_as(
        "SELECT is_superuser FROM users WHERE id = ?"
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(v,)| v == 1).unwrap_or(false))
}

/// Create a new user. Returns the new user's ID.
pub async fn create(
    pool: &DbPool,
    username: &str,
    password_hash: &str,
    is_superuser: i32,
) -> Result<i64, sqlx::Error> {
    sqlx::query(
        "INSERT INTO users (username, password_hash, is_superuser) VALUES (?, ?, ?)"
    )
    .bind(username)
    .bind(password_hash)
    .bind(is_superuser)
    .execute(pool)
    .await?;

    // AnyPool last_insert_id() can return None — fallback query
    let row: (i64,) = sqlx::query_as(
        "SELECT id FROM users WHERE username = ?"
    )
    .bind(username)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

/// Update a user's password hash.
pub async fn update_password(
    pool: &DbPool,
    user_id: i64,
    password_hash: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
        .bind(password_hash)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Delete a user by ID.
pub async fn delete(pool: &DbPool, user_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Update last_login timestamp for a user.
pub async fn update_last_login(pool: &DbPool, user_id: i64, timestamp: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE users SET last_login = ? WHERE id = ?")
        .bind(timestamp)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;

    #[tokio::test]
    async fn test_create_and_get_all_views() {
        let pool = create_test_pool().await;
        let id = create(&pool, "alice", "hash123", 0).await.unwrap();
        assert!(id > 0);

        let views = get_all_views(&pool).await.unwrap();
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].username, "alice");
        assert_eq!(views[0].is_superuser, 0);
    }

    #[tokio::test]
    async fn test_create_duplicate_username() {
        let pool = create_test_pool().await;
        create(&pool, "bob", "hash1", 0).await.unwrap();
        let result = create(&pool, "bob", "hash2", 0).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_is_superuser() {
        let pool = create_test_pool().await;
        let id = create(&pool, "admin", "hash", 1).await.unwrap();
        assert!(is_superuser(&pool, id).await.unwrap());

        let id2 = create(&pool, "user", "hash", 0).await.unwrap();
        assert!(!is_superuser(&pool, id2).await.unwrap());
    }

    #[tokio::test]
    async fn test_is_superuser_nonexistent() {
        let pool = create_test_pool().await;
        assert!(!is_superuser(&pool, 9999).await.unwrap());
    }

    #[tokio::test]
    async fn test_update_password() {
        let pool = create_test_pool().await;
        let id = create(&pool, "carol", "old_hash", 0).await.unwrap();
        update_password(&pool, id, "new_hash").await.unwrap();

        let user = get_by_id(&pool, id).await.unwrap().unwrap();
        assert_eq!(user.password_hash, "new_hash");
    }

    #[tokio::test]
    async fn test_delete() {
        let pool = create_test_pool().await;
        let id = create(&pool, "dave", "hash", 0).await.unwrap();
        delete(&pool, id).await.unwrap();

        let user = get_by_id(&pool, id).await.unwrap();
        assert!(user.is_none());
    }

    #[tokio::test]
    async fn test_update_last_login() {
        let pool = create_test_pool().await;
        let id = create(&pool, "eve", "hash", 0).await.unwrap();

        update_last_login(&pool, id, "2026-01-15 10:30:00").await.unwrap();
        let views = get_all_views(&pool).await.unwrap();
        assert_eq!(views[0].last_login, "2026-01-15 10:30:00");
    }
}
