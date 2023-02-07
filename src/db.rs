use super::{AddFeed, Article, Feed};
use anyhow::Result;
use futures::lock::Mutex;
use std::sync::Arc;
use tokio_postgres::{Client, Config, NoTls, Row};

pub static MAX_DATE: &str = "9999-12-31";
pub static MIN_ID: &str = "0";

#[derive(Default, Clone)]
pub struct Page {
    pub has_next: bool,
    pub has_prev: bool,
    pub curr: String,
    pub next: String,
    pub prev: String,
}

#[derive(Clone)]
pub struct QueryResponse {
    pub articles: Option<Vec<Article>>,
    pub feeds: Option<Vec<Feed>>,
    pub page: Page,
}

impl Page {
    fn new(mut next: Vec<Row>, prev: Vec<Row>, curr: String) -> Self {
        let mut hn = false;
        let mut n = "".to_string();
        match next.len() {
            5 => {
                next.remove(next.len() - 1);
                hn = true;
                n = next[next.len() - 1].get(5);
            }
            1..=4 => {
                hn = false;
                n = next[next.len() - 1].get(5);
            }
            _ => (),
        }

        let mut hp = false;
        let mut p = "".to_string();
        match prev.len() {
            5 => {
                hp = true;
                p = prev[0 + 1].get(5);
            }
            1..=4 => {
                hp = true;
                p = MAX_DATE.to_string();
            }
            _ => (),
        }

        Page {
            has_next: hn,
            has_prev: hp,
            next: n,
            prev: p,
            curr: curr,
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
    id SERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    site_url TEXT NOT NULL,
    feed_url TEXT NOT NULL UNIQUE,
    date_added TEXT NOT NULL,
    last_updated TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS articles (
    id SERIAL PRIMARY KEY,
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
        let query = "INSERT INTO FEEDS (name, site_url, feed_url, date_added, last_updated) VALUES ($1, $2, $3, $4, $5)";
        let tx = conn.transaction().await?;
        let stmt = tx.prepare(query).await?;
        let fta = Feed::new(f.feed_name, f.site_url, f.feed_url);
        tx.execute(
            &stmt,
            &[
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

    pub(crate) async fn get_feed_by_id(&self, feed_id: i32) -> Result<Feed> {
        let conn = &mut self.client.lock().await;
        let query = "SELECT * FROM feeds WHERE id = $1";
        let result = conn.query_one(query, &[&feed_id]).await?;
        Ok(Feed::from(&result))
    }

    pub(crate) async fn get_feeds(&self, pagination: String) -> Result<QueryResponse> {
        let conn = &mut self.client.lock().await;
        let id: i32 = pagination.parse()?;
        let next_query = "SELECT * FROM feeds WHERE id > $1 ORDER BY id ASC LIMIT 5";
        let next = conn.query(next_query, &[&id]).await?;

        let prev_query = "SELECT * FROM ( SELECT * FROM feeds WHERE id < $1 ORDER BY id DESC LIMIT 5 ) AS data ORDER BY id ASC";
        let prev = conn.query(prev_query, &[&id]).await?;

        let feeds = next.iter().map(|r| r.into()).collect();
        Ok(QueryResponse {
            page: Page::new(next, prev, pagination),
            feeds: Some(feeds),
            articles: None,
        })
    }

    pub(crate) async fn delete_feed(&self, id: i32) -> Result<()> {
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
        feed_id: i32,
    ) -> Result<()> {
        let conn = &mut self.client.lock().await;
        let tx = conn.transaction().await?;
        let query = "UPDATE feeds SET last_updated = $1 WHERE id = $2";
        tx.query(query, &[&timestamp, &feed_id]).await?;
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn add_articles<T>(&self, articles: T) -> Result<()>
    where
        T: Iterator<Item = Article>,
    {
        let conn = &mut self.client.lock().await;
        let tx = conn.transaction().await?;
        let query = "INSERT INTO articles (feed, title, link, author, published, read, favorited, read_date) VALUES ($1, $2, $3, $4, $5, $6, $7, $8) ON CONFLICT (link) DO NOTHING";
        let stmt = tx.prepare(query).await?;
        for article in articles {
            tx.execute(
                &stmt,
                &[
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

    pub(crate) async fn get_article_by_id(&self, id: i32) -> Result<Article> {
        let conn = &mut self.client.lock().await;
        let query = "SELECT * FROM articles WHERE id = $1";
        let row = conn.query_one(query, &[&id]).await?;
        let article: Article = Article::from(&row);
        Ok(article)
    }

    pub(crate) async fn get_unread_articles(&self, pagination: String) -> Result<QueryResponse> {
        let conn = &mut self.client.lock().await;

        let next_query ="SELECT * FROM articles WHERE read = false AND published < $1 ORDER BY published DESC LIMIT 5";
        let next = conn.query(next_query, &[&pagination]).await?;

        let prev_query = "SELECT * FROM ( SELECT * FROM articles WHERE read = false AND published > $1 ORDER BY published asc LIMIT 5 ) AS data ORDER BY published DESC";
        let prev = conn.query(prev_query, &[&pagination]).await?;

        let articles = next.iter().map(|r| r.into()).collect();

        Ok(QueryResponse {
            page: Page::new(next, prev, pagination),
            articles: Some(articles),
            feeds: None,
        })
    }

    pub(crate) async fn get_read_articles(&self, pagination: String) -> Result<QueryResponse> {
        let conn = &mut self.client.lock().await;

        let next_query = "SELECT * FROM articles WHERE read = true AND read_date < $1 ORDER BY read_date DESC LIMIT 5";
        let next = conn.query(next_query, &[&pagination.clone()]).await?;

        let prev_query = "select * FROM ( SELECT * FROM articles WHERE read_date > $1 ORDER BY published ASC LIMIT 5 ) AS data ORDER BY read_date DESC";
        let prev = conn.query(prev_query, &[&pagination.clone()]).await?;

        let articles = next.iter().map(|r| r.into()).collect();

        Ok(QueryResponse {
            page: Page::new(next, prev, pagination),
            articles: Some(articles),
            feeds: None,
        })
    }

    pub(crate) async fn get_favorited_articles(&self, pagination: String) -> Result<QueryResponse> {
        let conn = &mut self.client.lock().await;

        let next_query = "SELECT * FROM articles WHERE favorited = true AND published < $1 ORDER BY published DESC LIMIT 5";
        let next = conn.query(next_query, &[&pagination]).await?;

        let prev_query = "SELECT * FROM ( SELECT * FROM articles WHERE favorited = true AND published > $1 ORDER BY published asc LIMIT 5 ) AS data ORDER BY published DESC";
        let prev = conn.query(prev_query, &[&pagination]).await?;

        let articles = next.iter().map(|r| r.into()).collect();

        Ok(QueryResponse {
            page: Page::new(next, prev, pagination),
            articles: Some(articles),
            feeds: None,
        })
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

    pub(crate) async fn mark_article_favorite(&self, id: i32) -> Result<()> {
        let conn = &mut self.client.lock().await;
        let query = "UPDATE articles SET favorited = NOT favorited WHERE id = $1";
        let tx = conn.transaction().await?;
        tx.execute(query, &[&id]).await?;
        tx.commit().await?;
        Ok(())
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
