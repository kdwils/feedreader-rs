mod db;

use anyhow::Result;
use askama::Template;
use chrono::{DateTime, SecondsFormat, Utc};
use core::panic;
use feed_rs::parser;
use rweb::*;
use serde::{Deserialize, Serialize};
use std::{env, str::FromStr, vec};

#[derive(Deserialize, Serialize)]
struct Healthz {
    up: bool,
}

#[derive(Template)]
#[template(path = "feeds.html")]
struct FeedsTemplate {
    page: db::Page,
    feeds: Vec<Feed>,
}

#[derive(Template)]
#[template(path = "feed_list.html")]
struct FeedListTemplate {
    page: db::Page,
    feeds: Vec<Feed>,
}

#[derive(Template)]
#[template(path = "add_feed.html")]
struct AddFeedTemplate {}

#[derive(Template)]
#[template(path = "article_list.html")]
struct ArticleListTemplate {
    page: db::Page,
    articles: Vec<Article>,
}

#[derive(Template, Default)]
#[template(path = "article_base.html")]
struct ArticleBaseTemplate {
    article_filter: String,
    title: String,
    page: db::Page,
    articles: Vec<Article>,
}

#[derive(Debug)]
struct AppError(anyhow::Error);
impl rweb::reject::Reject for AppError {}

fn reject_anyhow(err: anyhow::Error) -> Rejection {
    warp::reject::custom(AppError(err))
}

#[derive(Debug)]
struct BadFilterError(String);
impl rweb::reject::Reject for BadFilterError {}

#[derive(Debug)]
struct BadActionError();
impl rweb::reject::Reject for BadActionError {}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Feed {
    id: i32,
    name: String,
    site_url: String,
    feed_url: String,
    date_added: String,
    last_updated: String,
}

impl Feed {
    pub fn new(name: String, site_url: String, feed_url: String) -> Self {
        Feed {
            id: 0,
            name,
            site_url,
            feed_url,
            date_added: Utc::now()
                .to_rfc3339_opts(SecondsFormat::Millis, true)
                .to_string(),
            last_updated: "-1".to_string(),
        }
    }
}

impl From<&tokio_postgres::Row> for Feed {
    fn from(row: &tokio_postgres::Row) -> Self {
        Feed {
            id: row.get(0),
            name: row.get(1),
            site_url: row.get(2),
            feed_url: row.get(3),
            date_added: row.get(4),
            last_updated: row.get(5),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct AddFeed {
    feed_name: String,
    site_url: String,
    feed_url: String,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Article {
    id: i32,
    feed: String,
    title: String,
    link: String,
    author: String,
    published: String,
    read: bool,
    favorited: bool,
    read_date: String,
}

impl Article {
    pub fn new(
        title: String,
        link: String,
        author: String,
        published: String,
        read: bool,
        favorited: bool,
    ) -> Self {
        Article {
            id: 0,
            feed: "".to_string(),
            title,
            link,
            author,
            published: match DateTime::parse_from_rfc2822(published.as_str()) {
                Ok(dt) => dt.to_rfc3339_opts(SecondsFormat::Secs, true).to_string(),
                Err(_) => published,
            },
            read,
            favorited,
            read_date: "-1".to_string(),
        }
    }

    pub fn rfc3339_timestamp() -> String {
        Utc::now()
            .to_rfc3339_opts(SecondsFormat::Millis, true)
            .to_string()
    }

    pub fn rfc3339_timestamp_to_human(timestamp: String) -> String {
        match DateTime::parse_from_rfc3339(timestamp.as_str()) {
            Ok(dt) => dt.format("%m/%d/%Y").to_string(),
            Err(_) => timestamp,
        }
    }
}

impl From<&tokio_postgres::Row> for Article {
    fn from(row: &tokio_postgres::Row) -> Self {
        Article {
            id: row.get(0),
            feed: row.get(1),
            title: row.get(2),
            link: row.get(3),
            author: row.get(4),
            published: Article::rfc3339_timestamp_to_human(row.get(5)),
            read: row.get(6),
            favorited: row.get(7),
            read_date: Article::rfc3339_timestamp_to_human(row.get(8)),
        }
    }
}

impl From<&feed_rs::model::Entry> for Article {
    fn from(value: &feed_rs::model::Entry) -> Self {
        let title = match value.title.clone() {
            Some(text) => text.content.to_string(),
            None => "".to_string(),
        };

        let link = value
            .links
            .iter()
            .take(1)
            .map(|l| l.href.to_string())
            .next()
            .unwrap_or_else(|| "".to_string());

        let author = value
            .authors
            .iter()
            .take(1)
            .map(|p| p.name.to_string())
            .next()
            .unwrap_or_else(|| "".to_string());

        let timestamp = if let Some(_published) = value.published {
            value.published
        } else if let Some(_updated) = value.updated {
            value.updated
        } else {
            None
        };

        let published = match timestamp {
            Some(ts) => ts.to_rfc3339_opts(SecondsFormat::Millis, true),
            None => "".to_string(),
        };

        Article::new(title, link, author, published, false, false)
    }
}

#[tokio::main]
async fn main() {
    let db_username = env::var("POSTGRES_USERNAME").unwrap();
    let db_password = env::var("POSTGRES_PASSWORD").unwrap();
    let db_host = env::var("POSTGRES_HOST").unwrap_or("0.0.0.0".to_string());
    let db_port = env::var("POSTGRES_PORT")
        .unwrap_or("5432".to_string())
        .parse()
        .unwrap();

    let store = db::connection(
        db_username.as_str(),
        db_password.as_str(),
        db_host.as_str(),
        db_port,
    )
    .await
    .unwrap();

    match store.init().await {
        Ok(_) => (),
        Err(e) => panic!("could not init db: {}", e.to_string()),
    }

    let cors = warp::cors()
        .allow_any_origin()
        .allow_methods(vec!["GET", "HEAD", "POST", "DELETE"]);

    let feed_routes = healthz()
        .or(index(store.clone()))
        .or(favorited(store.clone()))
        .or(history(store.clone()))
        .or(get_articles(store.clone()))
        .or(mark_article_read(store.clone()))
        .or(mark_article_favorite(store.clone()))
        .or(add_feed(store.clone()))
        .or(feeds_html(store.clone()))
        .or(delete_feed(store.clone()))
        .or(add_feed_html())
        .or(refresh_feed(store.clone()))
        .with(cors);

    serve(feed_routes).run(([0, 0, 0, 0], 8080)).await;
}

#[get("/healthz")]
fn healthz() -> Json<Healthz> {
    Healthz { up: true }.into()
}

#[get("/")]
async fn index(#[data] store: db::Storage) -> Result<ArticleBaseTemplate, Rejection> {
    let qr = store
        .get_unread_articles(db::MAX_DATE.to_string())
        .await
        .map_err(reject_anyhow)?;

    Ok(ArticleBaseTemplate {
        page: qr.page,
        title: ArticleFilter::Unread.to_string(),
        article_filter: ArticleFilter::Unread.to_string(),
        articles: match qr.articles {
            Some(articles) => articles,
            None => vec![],
        },
    })
}

#[get("/favorites.html")]
async fn favorited(#[data] store: db::Storage) -> Result<ArticleBaseTemplate, Rejection> {
    let qr = store
        .get_favorited_articles(db::MAX_DATE.to_string())
        .await
        .map_err(reject_anyhow)?;

    Ok(ArticleBaseTemplate {
        page: qr.page,
        title: "favorites".to_string(),
        article_filter: ArticleFilter::Favorite.to_string(),
        articles: qr.articles.unwrap_or(vec![]),
    })
}

#[get("/history.html")]
async fn history(#[data] store: db::Storage) -> Result<ArticleBaseTemplate, Rejection> {
    let qr = store
        .get_read_articles(db::MAX_DATE.to_string())
        .await
        .map_err(reject_anyhow)?;

    Ok(ArticleBaseTemplate {
        page: qr.page,
        title: "history".to_string(),
        article_filter: ArticleFilter::Read.to_string(),
        articles: qr.articles.unwrap_or(vec![]),
    })
}

#[get("/feeds.html")]
async fn feeds_html(#[data] db: db::Storage) -> Result<FeedsTemplate, Rejection> {
    let qr = db
        .get_feeds(db::MIN_ID.to_string())
        .await
        .map_err(reject_anyhow)?;

    Ok(FeedsTemplate {
        page: qr.page,
        feeds: qr.feeds.unwrap_or(vec![]),
    })
}

#[get("/add_feed.html")]
async fn add_feed_html() -> Result<AddFeedTemplate, Rejection> {
    Ok(AddFeedTemplate {})
}

#[post("/feeds")]
async fn add_feed(
    #[form] feed: AddFeed,
    #[data] store: db::Storage,
    #[header = "pagination"] pagination: String,
) -> Result<FeedsTemplate, Rejection> {
    store.add_feed(feed).await.map_err(reject_anyhow)?;
    let qr = store.get_feeds(pagination).await.map_err(reject_anyhow)?;

    Ok(FeedsTemplate {
        page: qr.page,
        feeds: qr.feeds.unwrap_or(vec![]),
    })
}

#[delete("/feeds/{id}")]
async fn delete_feed(
    #[data] store: db::Storage,
    id: i32,
    #[header = "pagination"] pagination: String,
) -> Result<FeedListTemplate, Rejection> {
    store.delete_feed(id).await.map_err(reject_anyhow)?;
    let qr = store.get_feeds(pagination).await.map_err(reject_anyhow)?;

    Ok(FeedListTemplate {
        page: qr.page,
        feeds: qr.feeds.unwrap_or(vec![]),
    })
}

#[post("/feeds/{id}/refresh")]
async fn refresh_feed(
    id: i32,
    #[data] store: db::Storage,
    #[header = "pagination"] pagination: String,
) -> Result<FeedListTemplate, Rejection> {
    let feed = store
        .get_feed_by_id(id.clone())
        .await
        .map_err(reject_anyhow)?;

    let content = reqwest::get(feed.feed_url)
        .await
        .map_err(|_| warp::reject())?
        .bytes()
        .await
        .map_err(|_| warp::reject())?;

    let parsed_feed = parser::parse(content.reader()).map_err(|_| warp::reject())?;
    let articles: Vec<Article> = parsed_feed
        .entries
        .iter()
        .map(|e| {
            let mut o: Article = e.into();
            o.feed = feed.name.clone();
            o
        })
        .collect();

    store
        .add_articles(articles.clone().into_iter())
        .await
        .map_err(reject_anyhow)?;

    let timestamp = chrono::Utc::now()
        .to_rfc3339_opts(SecondsFormat::Secs, true)
        .to_string();

    match store.update_feed_last_updated(timestamp, id.clone()).await {
        Ok(_) => (), // does not matter if this works or not
        Err(e) => println!("could not update feed timestamp: {}", e.to_string()),
    }

    let qr = store.get_feeds(pagination).await.map_err(reject_anyhow)?;

    Ok(FeedListTemplate {
        page: qr.page,
        feeds: qr.feeds.unwrap_or(vec![]),
    })
}

#[post("/articles/{article_id}/read")]
async fn mark_article_read(
    article_id: i32,
    #[data] store: db::Storage,
    #[header = "pagination"] pagination: String,
    #[header = "article_filter"] article_filter: String,
) -> Result<ArticleListTemplate, Rejection> {
    let article = store
        .get_article_by_id(article_id.clone())
        .await
        .map_err(reject_anyhow)?;

    store
        .mark_article_read(article)
        .await
        .map_err(reject_anyhow)?;

    let filter = ArticleFilter::from_str(article_filter.as_str())?;

    let qr = filter
        .list_articles(store.clone(), pagination.clone())
        .await
        .map_err(reject_anyhow)?;

    Ok(ArticleListTemplate {
        page: qr.page,
        articles: qr.articles.unwrap_or(vec![]),
    })
}

#[post("/articles/{article_id}/favorite")]
async fn mark_article_favorite(
    article_id: i32,
    #[header = "pagination"] pagination: String,
    #[header = "article_filter"] article_filter: String,
    #[data] store: db::Storage,
) -> Result<ArticleListTemplate, Rejection> {
    store
        .mark_article_favorite(article_id)
        .await
        .map_err(reject_anyhow)?;

    let filter = ArticleFilter::from_str(article_filter.as_str())?;

    let qr = filter
        .list_articles(store.clone(), pagination)
        .await
        .map_err(reject_anyhow)?;

    Ok(ArticleListTemplate {
        page: qr.page,
        articles: qr.articles.unwrap_or(vec![]),
    })
}

#[get("/articles")]
async fn get_articles(
    #[data] store: db::Storage,
    #[header = "pagination"] pagination: String,
    #[header = "article_filter"] article_filter: String,
) -> Result<ArticleListTemplate, Rejection> {
    let filter = ArticleFilter::from_str(article_filter.as_str())?;

    let qr = filter
        .list_articles(store.clone(), pagination)
        .await
        .map_err(reject_anyhow)?;

    Ok(ArticleListTemplate {
        page: qr.page,
        articles: qr.articles.unwrap_or(vec![]),
    })
}

#[derive(Debug)]
enum ArticleFilter {
    Unread,
    Favorite,
    Read,
}

impl FromStr for ArticleFilter {
    type Err = BadFilterError;
    fn from_str(s: &str) -> Result<ArticleFilter, BadFilterError> {
        match s {
            "unread" => Ok(ArticleFilter::Unread),
            "favorite" => Ok(ArticleFilter::Favorite),
            "read" => Ok(ArticleFilter::Read),
            _ => Err(BadFilterError(
                format!("bad article filter: {}", s).to_string(),
            )),
        }
    }
}

impl ArticleFilter {
    async fn list_articles(
        self,
        store: db::Storage,
        pagination: String,
    ) -> Result<db::QueryResponse> {
        match self {
            ArticleFilter::Unread => return store.get_unread_articles(pagination).await,
            ArticleFilter::Favorite => return store.get_favorited_articles(pagination).await,
            ArticleFilter::Read => return store.get_favorited_articles(pagination).await,
        }
    }

    fn to_string(&self) -> String {
        match self {
            ArticleFilter::Read => "read".to_string(),
            ArticleFilter::Favorite => "favorite".to_string(),
            ArticleFilter::Unread => "unread".to_string(),
        }
    }
}
