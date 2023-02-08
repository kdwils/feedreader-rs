use super::{AddFeed, Article, Feed};
use anyhow::Result;
use futures::lock::Mutex;
use std::str::FromStr;
use std::sync::Arc;

use tokio_postgres::{Client, Config, NoTls, Row};

pub static MAX_DATE: &str = "9999-12-31";

const LIMIT: usize = 4;
const LIMIT_UPPER_BOUND: usize = LIMIT + 1;
const LIMIT_LOWER_BOUND: usize = LIMIT - 1;

pub enum Filter {
    Unread,
    Favorite,
    Read,
}

impl ToString for Filter {
    fn to_string(&self) -> String {
        match self {
            Filter::Read => "read".to_string(),
            Filter::Favorite => "favorite".to_string(),
            Filter::Unread => "unread".to_string(),
        }
    }
}

impl FromStr for Filter {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Filter> {
        match s {
            "unread" => Ok(Filter::Unread),
            "favorite" => Ok(Filter::Favorite),
            "read" => Ok(Filter::Read),
            _ => Err(anyhow::Error::msg(format!("bad filter type: {}", s))),
        }
    }
}

enum Ordering {
    Ascending,
    Descending,
}

impl ToString for Ordering {
    fn to_string(&self) -> String {
        match *self {
            Ordering::Ascending => "ASC".to_string(),
            Ordering::Descending => "DESC".to_string(),
        }
    }
}

enum PaginationField {
    Id,
    Published,
    ReadDate,
}

impl PaginationField {
    fn index(self) -> usize {
        match self {
            PaginationField::Id => 0,
            PaginationField::Published => 5,
            PaginationField::ReadDate => 8,
        }
    }
}

pub struct Page {
    pub cursor: Cursor,
    pub items: Vec<Row>,
}

impl Page {
    fn new(next: Vec<Row>, prev: Vec<Row>, curr: String, paginated_field: PaginationField) -> Page {
        Page {
            cursor: Cursor::new(next.as_slice(), prev, curr, paginated_field.index()),
            items: Cursor::items(next),
        }
    }
}

#[derive(Default, Clone)]
pub struct Cursor {
    pub has_next: bool,
    pub has_prev: bool,
    pub curr: String,
    pub next: String,
    pub prev: String,
}

impl Cursor {
    fn new(next: &[Row], prev: Vec<Row>, curr: String, index: usize) -> Self {
        let (hn, n) = match next.len() {
            // next contains the elements for the next page, we only need elements up to the limit as the last is used to confirm there is another page
            LIMIT_UPPER_BOUND => (true, next[next.len() - 1 - 1].get(index)),
            1..=LIMIT_LOWER_BOUND => (false, next[next.len() - 1].get(index)),
            _ => (false, "".to_string()),
        };

        let (hp, p) = match prev.len() {
            LIMIT_UPPER_BOUND => (true, prev[0 + 1].get(index)),
            1..=LIMIT_LOWER_BOUND => (true, MAX_DATE.to_string()),
            _ => (false, "".to_string()),
        };

        Cursor {
            has_next: hn,
            has_prev: hp,
            next: n,
            prev: p,
            curr: curr,
        }
    }

    fn items(next: Vec<Row>) -> Vec<Row> {
        let mut items = next;
        match items.len() {
            LIMIT_UPPER_BOUND => {
                // if we have more elements than the limit, another page exists and we only need the len of LIMIT elements
                items.pop();
                items
            }
            _ => items,
        }
    }
}

#[derive(Clone)]
pub struct Storage {
    client: Arc<Mutex<Client>>,
}

impl Storage {
    pub(crate) async fn init(&self) -> Result<()> {
        let conn = self.client.lock().await;
        let query = r#"
CREATE TABLE IF NOT EXISTS feeds (
    id TEXT NOT NULL,
    name TEXT NOT NULL,
    site_url TEXT NOT NULL,
    feed_url TEXT NOT NULL UNIQUE,
    date_added TEXT NOT NULL,
    last_updated TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS articles (
    id TEXT NOT NULL,
    feed TEXT NOT NULL,
    title TEXT NOT NULL,
    link TEXT NOT NULL UNIQUE,
    author TEXT NOT NULL,
    published TEXT NOT NULL,
    read BOOLEAN NOT NULL,
    favorited BOOLEAN NOT NULL,
    read_date TEXT NOT NULL
);"#;
        conn.batch_execute(query).await?;
        Ok(())
    }

    pub(crate) async fn add_feed(&self, f: AddFeed) -> Result<Feed> {
        let conn = &mut self.client.lock().await;
        let query = "INSERT INTO FEEDS (id, name, site_url, feed_url, date_added, last_updated) VALUES ($1, $2, $3, $4, $5, $6)";
        let tx = conn.transaction().await?;
        let stmt = tx.prepare(query).await?;
        let fta = Feed::new(f.feed_name, f.site_url, f.feed_url);
        tx.execute(
            &stmt,
            &[
                &fta.id,
                &fta.name,
                &fta.site_url,
                &fta.feed_url,
                &fta.date_added,
                &fta.last_updated,
            ],
        )
        .await?;
        tx.commit().await?;

        Ok(fta)
    }

    pub(crate) async fn get_feed_by_id(&self, id: String) -> Result<Feed> {
        let conn = &mut self.client.lock().await;
        let query = "SELECT * FROM feeds WHERE id = $1";
        let result = conn.query_one(query, &[&id]).await?;
        Ok(Feed::from(&result))
    }

    pub(crate) async fn get_feeds(&self, pagination: String) -> Result<Page> {
        let conn = &mut self.client.lock().await;
        let next_query = format!(
            "SELECT * FROM feeds WHERE date_added < $1 ORDER BY id {} LIMIT {}",
            Ordering::Descending.to_string(),
            LIMIT_UPPER_BOUND
        );
        let next = conn.query(next_query.as_str(), &[&pagination]).await?;

        let prev_query = format!("SELECT * FROM ( SELECT * FROM feeds WHERE date_added > $1 ORDER BY id {} LIMIT {} ) AS data ORDER BY date_added {}", Ordering::Ascending.to_string(), LIMIT_UPPER_BOUND, Ordering::Descending.to_string());
        let prev = conn.query(prev_query.as_str(), &[&pagination]).await?;

        Ok(Page::new(next, prev, pagination, PaginationField::Id))
    }

    pub(crate) async fn delete_feed(&self, id: String) -> Result<()> {
        let conn = &mut self.client.lock().await;
        let query = "DELETE FROM feeds WHERE id = $1";
        let tx = conn.transaction().await?;
        tx.execute(query, &[&id]).await?;
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn update_feed_last_updated(
        &self,
        timestamp: String,
        id: String,
    ) -> Result<()> {
        let conn = &mut self.client.lock().await;
        let tx = conn.transaction().await?;
        let query = "UPDATE feeds SET last_updated = $1 WHERE id = $2";
        tx.query(query, &[&timestamp, &id]).await?;
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn add_articles<T>(&self, articles: T) -> Result<()>
    where
        T: Iterator<Item = Article>,
    {
        let conn = &mut self.client.lock().await;
        let tx = conn.transaction().await?;
        let query = "INSERT INTO articles (id, feed, title, link, author, published, read, favorited, read_date) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) ON CONFLICT (link) DO NOTHING";
        let stmt = tx.prepare(query).await?;
        for article in articles {
            tx.execute(
                &stmt,
                &[
                    &article.id,
                    &article.feed,
                    &article.title,
                    &article.link,
                    &article.author,
                    &article.published,
                    &article.read,
                    &article.favorited,
                    &article.read_date,
                ],
            )
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn get_article_by_id(&self, id: String) -> Result<Article> {
        let conn = &mut self.client.lock().await;
        let query = "SELECT * FROM articles WHERE id = $1";
        let row = conn.query_one(query, &[&id]).await?;
        Ok(Article::from(&row))
    }

    pub(crate) async fn get_unread_articles(&self, pagination: String) -> Result<Page> {
        let conn = &mut self.client.lock().await;

        let next_query =format!("SELECT * FROM articles WHERE read = false AND published < $1 ORDER BY published {} LIMIT {}", Ordering::Descending.to_string(), LIMIT_UPPER_BOUND);
        let next = conn.query(next_query.as_str(), &[&pagination]).await?;

        let prev_query = format!("SELECT * FROM ( SELECT * FROM articles WHERE read = false AND published > $1 ORDER BY published {} LIMIT {} ) AS data ORDER BY published {}", Ordering::Ascending.to_string(), LIMIT_UPPER_BOUND, Ordering::Descending.to_string());
        let prev = conn.query(prev_query.as_str(), &[&pagination]).await?;

        Ok(Page::new(
            next,
            prev,
            pagination,
            PaginationField::Published,
        ))
    }

    pub(crate) async fn get_read_articles(&self, pagination: String) -> Result<Page> {
        let conn = &mut self.client.lock().await;

        let next_query = format!("SELECT * FROM articles WHERE read = true AND read_date < $1 ORDER BY read_date {} LIMIT {}", Ordering::Descending.to_string(), LIMIT_UPPER_BOUND);
        let next = conn
            .query(next_query.as_str(), &[&pagination.clone()])
            .await?;

        let prev_query = format!("SELECT * FROM ( SELECT * FROM articles WHERE read = true AND read_date > $1 ORDER BY read_date {} LIMIT {} ) AS data ORDER BY read_date {}", Ordering::Ascending.to_string(), LIMIT_UPPER_BOUND, Ordering::Descending.to_string());
        let prev = conn
            .query(prev_query.as_str(), &[&pagination.clone()])
            .await?;

        Ok(Page::new(next, prev, pagination, PaginationField::ReadDate))
    }

    pub(crate) async fn get_favorited_articles(&self, pagination: String) -> Result<Page> {
        let conn = &mut self.client.lock().await;

        let next_query = format!("SELECT * FROM articles WHERE favorited = true AND published < $1 ORDER BY published {} LIMIT {}", Ordering::Descending.to_string(), LIMIT_UPPER_BOUND);
        let next = conn.query(next_query.as_str(), &[&pagination]).await?;

        let prev_query = format!("SELECT * FROM ( SELECT * FROM articles WHERE favorited = true AND published > $1 ORDER BY published {} LIMIT {} ) AS data ORDER BY published {}", Ordering::Ascending.to_string(), LIMIT_UPPER_BOUND, Ordering::Descending.to_string());
        let prev = conn.query(prev_query.as_str(), &[&pagination]).await?;

        Ok(Page::new(
            next,
            prev,
            pagination,
            PaginationField::Published,
        ))
    }

    pub(crate) async fn mark_article_read(&self, a: Article) -> Result<()> {
        let conn = &mut self.client.lock().await;
        let timestamp = match a.read {
            true => "-1".to_string(),
            false => Article::rfc3339_timestamp(),
        };

        let query = "UPDATE articles SET read = NOT read, read_date = $1 WHERE id = $2";
        let tx = conn.transaction().await?;
        tx.execute(query, &[&timestamp, &a.clone().id]).await?;
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn mark_article_favorite(&self, id: String) -> Result<()> {
        let conn = &mut self.client.lock().await;
        let query = "UPDATE articles SET favorited = NOT favorited WHERE id = $1";
        let tx = conn.transaction().await?;
        tx.execute(query, &[&id]).await?;
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn filter(self, filter: Filter, pagination: String) -> Result<Page> {
        match filter {
            Filter::Unread => return self.get_unread_articles(pagination).await,
            Filter::Favorite => return self.get_favorited_articles(pagination).await,
            Filter::Read => return self.get_read_articles(pagination).await,
        }
    }
}

pub async fn connection(username: &str, password: &str, host: &str, port: u16) -> Result<Storage> {
    let (client, connection) = Config::new()
        .user(username)
        .password(password)
        .host(host)
        .dbname("feedreader")
        .port(port)
        .connect(NoTls)
        .await?;

    tokio::spawn(async move {
        if let Err(error) = connection.await {
            eprintln!("Connection error: {}", error);
        }
    });

    Ok(Storage {
        client: Arc::new(Mutex::new(client)),
    })
}
