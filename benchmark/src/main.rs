use deser::{Deserialize, Serialize};

fn input_json() -> String {
    std::fs::read_to_string("benches/twitter.json").unwrap()
}

fn input_struct() -> Twitter {
    let j = input_json();
    deser_json::from_str(&j).unwrap()
}

fn timeit<F: FnMut()>(name: &str, iterations: usize, mut f: F) {
    // warmup
    for _ in 0..iterations / 10 + 1 {
        f();
    }
    let mut best = f64::MAX;
    for _ in 0..10 {
        let start = std::time::Instant::now();
        for _ in 0..iterations / 10 + 1 {
            f();
        }
        let per_iter = start.elapsed().as_secs_f64() / (iterations / 10 + 1) as f64;
        best = best.min(per_iter);
    }
    println!("{:<20} {:>10.1} us", name, best * 1e6);
}

#[cfg(feature = "count-allocs")]
mod counting {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicUsize, Ordering};

    pub static ALLOCS: AtomicUsize = AtomicUsize::new(0);

    pub struct Counting;

    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
            System.alloc(layout)
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            System.dealloc(ptr, layout)
        }
        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
            System.realloc(ptr, layout, new_size)
        }
    }

    #[global_allocator]
    static GLOBAL: Counting = Counting;

    pub fn count<F: FnOnce()>(name: &str, f: F) {
        let before = ALLOCS.load(Ordering::Relaxed);
        f();
        println!("{:<20} {:>10} allocs", name, ALLOCS.load(Ordering::Relaxed) - before);
    }
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_default();
    let iterations: usize = std::env::args()
        .nth(2)
        .and_then(|x| x.parse().ok())
        .unwrap_or(100);

    let j = input_json();
    let s = input_struct();
    match mode.as_str() {
        // plain loops for profiling
        "de" => {
            for _ in 0..iterations {
                deser_json::from_str::<Twitter>(&j).unwrap();
            }
        }
        "de-serde" => {
            for _ in 0..iterations {
                serde_json::from_str::<Twitter>(&j).unwrap();
            }
        }
        "ser-serde" => {
            for _ in 0..iterations {
                serde_json::to_string(&s).unwrap();
            }
        }
        "ser" => {
            for _ in 0..iterations {
                deser_json::to_string(&s).unwrap();
            }
        }
        "ignore" => {
            for _ in 0..iterations {
                deser_json::from_str::<Ignore>(&j).unwrap();
            }
        }
        #[cfg(feature = "count-allocs")]
        "allocs" => {
            counting::count("de deser", || {
                std::hint::black_box(deser_json::from_str::<Twitter>(&j).unwrap());
            });
            counting::count("de serde", || {
                std::hint::black_box(serde_json::from_str::<Twitter>(&j).unwrap());
            });
            counting::count("ignore deser", || {
                std::hint::black_box(deser_json::from_str::<Ignore>(&j).unwrap());
            });
            counting::count("ser deser", || {
                std::hint::black_box(deser_json::to_string(&s).unwrap());
            });
            counting::count("ser serde", || {
                std::hint::black_box(serde_json::to_string(&s).unwrap());
            });
        }
        // timings
        _ => {
            let n = iterations.max(1000);
            timeit("de deser", n, || {
                std::hint::black_box(deser_json::from_str::<Twitter>(&j).unwrap());
            });
            timeit("de serde", n, || {
                std::hint::black_box(serde_json::from_str::<Twitter>(&j).unwrap());
            });
            timeit("ignore deser", n, || {
                std::hint::black_box(deser_json::from_str::<Ignore>(&j).unwrap());
            });
            timeit("ignore serde", n, || {
                std::hint::black_box(serde_json::from_str::<serde::de::IgnoredAny>(&j).unwrap());
            });
            timeit("ser deser", n, || {
                std::hint::black_box(deser_json::to_string(&s).unwrap());
            });
            timeit("ser serde", n, || {
                std::hint::black_box(serde_json::to_string(&s).unwrap());
            });
        }
    }
}

/// Accepts and ignores any value.
struct Ignore;

impl deser::Deserialize for Ignore {
    fn deserialize_into(out: &mut Option<Self>) -> deser::de::SinkHandle<'_> {
        *out = Some(Ignore);
        deser::de::SinkHandle::null()
    }
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize)]
struct Twitter {
    statuses: Vec<Status>,
    search_metadata: SearchMetadata,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize)]
struct Status {
    metadata: Metadata,
    created_at: String,
    id: u64,
    id_str: String,
    text: String,
    source: String,
    truncated: bool,
    in_reply_to_status_id: Option<u64>,
    in_reply_to_status_id_str: Option<String>,
    in_reply_to_user_id: Option<u32>,
    in_reply_to_user_id_str: Option<String>,
    in_reply_to_screen_name: Option<String>,
    user: User,
    geo: (),
    coordinates: (),
    place: (),
    contributors: (),
    retweeted_status: Option<Box<Status>>,
    retweet_count: u32,
    favorite_count: u32,
    entities: StatusEntities,
    favorited: bool,
    retweeted: bool,
    possibly_sensitive: Option<bool>,
    lang: String,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize)]
struct Metadata {
    result_type: String,
    iso_language_code: String,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize)]
struct User {
    id: u32,
    id_str: String,
    name: String,
    screen_name: String,
    location: String,
    description: String,
    url: Option<String>,
    entities: UserEntities,
    protected: bool,
    followers_count: u32,
    friends_count: u32,
    listed_count: u32,
    created_at: String,
    favourites_count: u32,
    utc_offset: Option<i32>,
    time_zone: Option<String>,
    geo_enabled: bool,
    verified: bool,
    statuses_count: u32,
    lang: String,
    contributors_enabled: bool,
    is_translator: bool,
    is_translation_enabled: bool,
    profile_background_color: String,
    profile_background_image_url: String,
    profile_background_image_url_https: String,
    profile_background_tile: bool,
    profile_image_url: String,
    profile_image_url_https: String,
    profile_banner_url: Option<String>,
    profile_link_color: String,
    profile_sidebar_border_color: String,
    profile_sidebar_fill_color: String,
    profile_text_color: String,
    profile_use_background_image: bool,
    default_profile: bool,
    default_profile_image: bool,
    following: bool,
    follow_request_sent: bool,
    notifications: bool,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize)]
struct UserEntities {
    url: Option<UserUrl>,
    description: UserEntitiesDescription,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize)]
struct UserUrl {
    urls: Vec<Url>,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize)]
struct Url {
    url: String,
    expanded_url: String,
    display_url: String,
    indices: Indices,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize)]
struct UserEntitiesDescription {
    urls: Vec<Url>,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize)]
struct StatusEntities {
    hashtags: Vec<Hashtag>,
    symbols: Vec<()>,
    urls: Vec<Url>,
    user_mentions: Vec<UserMention>,
    media: Option<Vec<Media>>,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize)]
struct Hashtag {
    text: String,
    indices: Indices,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize)]
struct UserMention {
    screen_name: String,
    name: String,
    id: u32,
    id_str: String,
    indices: Indices,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize)]
struct Media {
    id: u64,
    id_str: String,
    indices: Indices,
    media_url: String,
    media_url_https: String,
    url: String,
    display_url: String,
    expanded_url: String,
    #[deser(rename = "type")]
    #[serde(rename = "type")]
    media_type: String,
    sizes: Sizes,
    source_status_id: Option<u64>,
    source_status_id_str: Option<String>,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize)]
struct Sizes {
    medium: Size,
    small: Size,
    thumb: Size,
    large: Size,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize)]
struct Size {
    w: u16,
    h: u16,
    resize: String,
}

type Indices = (u8, u8);

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize)]
struct SearchMetadata {
    completed_in: f32,
    max_id: u64,
    max_id_str: String,
    next_results: String,
    query: String,
    refresh_url: String,
    count: u8,
    since_id: u64,
    since_id_str: String,
}
