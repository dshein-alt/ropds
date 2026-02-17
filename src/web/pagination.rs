use serde::Serialize;

/// Half-width of the page number window shown in pagination bar.
const HALF_PAGES: i32 = 3;

#[derive(Debug, Clone, Serialize)]
pub struct Pagination {
    pub current_page: i32,
    pub total_pages: i32,
    pub total_items: i64,
    pub has_previous: bool,
    pub has_next: bool,
    pub previous_page: i32,
    pub next_page: i32,
    /// Page numbers to display in the pagination bar.
    pub page_range: Vec<i32>,
}

impl Pagination {
    pub fn new(current_page: i32, items_per_page: i32, total_items: i64) -> Self {
        let total_pages = if total_items == 0 {
            1
        } else {
            ((total_items as f64) / (items_per_page as f64)).ceil() as i32
        };
        let current_page = current_page.clamp(0, total_pages - 1);
        let has_previous = current_page > 0;
        let has_next = current_page < total_pages - 1;

        let start = (current_page - HALF_PAGES).max(0);
        let end = (current_page + HALF_PAGES).min(total_pages - 1);
        let page_range: Vec<i32> = (start..=end).collect();

        Self {
            current_page,
            total_pages,
            total_items,
            has_previous,
            has_next,
            previous_page: (current_page - 1).max(0),
            next_page: (current_page + 1).min(total_pages - 1),
            page_range,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_page() {
        let p = Pagination::new(0, 30, 10);
        assert_eq!(p.total_pages, 1);
        assert!(!p.has_previous);
        assert!(!p.has_next);
        assert_eq!(p.page_range, vec![0]);
    }

    #[test]
    fn test_multiple_pages() {
        let p = Pagination::new(2, 30, 150);
        assert_eq!(p.total_pages, 5);
        assert!(p.has_previous);
        assert!(p.has_next);
        assert_eq!(p.previous_page, 1);
        assert_eq!(p.next_page, 3);
        assert_eq!(p.page_range, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn test_last_page() {
        let p = Pagination::new(4, 30, 150);
        assert_eq!(p.current_page, 4);
        assert!(p.has_previous);
        assert!(!p.has_next);
    }

    #[test]
    fn test_empty() {
        let p = Pagination::new(0, 30, 0);
        assert_eq!(p.total_pages, 1);
        assert!(!p.has_previous);
        assert!(!p.has_next);
    }

    #[test]
    fn test_clamp_beyond_last() {
        let p = Pagination::new(999, 30, 60);
        assert_eq!(p.current_page, 1);
        assert!(!p.has_next);
    }
}
