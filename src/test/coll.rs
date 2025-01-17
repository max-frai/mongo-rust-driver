use bson::{doc, Bson, Document};
use lazy_static::lazy_static;

use crate::{
    error::ErrorKind,
    event::command::CommandStartedEvent,
    options::{AggregateOptions, FindOptions, InsertManyOptions, UpdateOptions},
    test::{
        util::{drop_collection, CommandEvent, EventClient},
        CLIENT,
        LOCK,
    },
};

#[test]
#[function_name::named]
fn count() {
    let _guard = LOCK.run_concurrently();

    let coll = CLIENT.init_db_and_coll(function_name!(), function_name!());

    assert_eq!(coll.estimated_document_count(None).unwrap(), 0);

    let _ = coll.insert_one(doc! { "x": 1 }, None).unwrap();
    assert_eq!(coll.estimated_document_count(None).unwrap(), 1);

    let result = coll
        .insert_many((1..4).map(|i| doc! { "x": i }).collect::<Vec<_>>(), None)
        .unwrap();
    assert_eq!(result.inserted_ids.len(), 3);
    assert_eq!(coll.estimated_document_count(None).unwrap(), 4);
}

#[test]
#[function_name::named]
fn find() {
    let _guard = LOCK.run_concurrently();

    let coll = CLIENT.init_db_and_coll(function_name!(), function_name!());

    let result = coll
        .insert_many((0i32..5).map(|i| doc! { "x": i }).collect::<Vec<_>>(), None)
        .unwrap();
    assert_eq!(result.inserted_ids.len(), 5);

    for (i, result) in coll.find(None, None).unwrap().enumerate() {
        let doc = result.unwrap();
        if i > 4 {
            panic!("expected 4 result, got {}", i);
        }

        assert_eq!(doc.len(), 2);
        assert!(doc.contains_key("_id"));
        assert_eq!(doc.get("x"), Some(&Bson::I32(i as i32)));
    }
}

#[test]
#[function_name::named]
fn update() {
    let _guard = LOCK.run_concurrently();

    let coll = CLIENT.init_db_and_coll(function_name!(), function_name!());

    let result = coll
        .insert_many((0i32..5).map(|_| doc! { "x": 3 }).collect::<Vec<_>>(), None)
        .unwrap();
    assert_eq!(result.inserted_ids.len(), 5);

    let update_one_results = coll
        .update_one(doc! {"x": 3}, doc! {"$set": { "x": 5 }}, None)
        .unwrap();
    assert_eq!(update_one_results.modified_count, 1);
    assert!(update_one_results.upserted_id.is_none());

    let update_many_results = coll
        .update_many(doc! {"x": 3}, doc! {"$set": { "x": 4}}, None)
        .unwrap();
    assert_eq!(update_many_results.modified_count, 4);
    assert!(update_many_results.upserted_id.is_none());

    let options = UpdateOptions::builder().upsert(true).build();
    let upsert_results = coll
        .update_one(doc! {"b": 7}, doc! {"$set": { "b": 7 }}, options)
        .unwrap();
    assert_eq!(upsert_results.modified_count, 0);
    assert!(upsert_results.upserted_id.is_some());
}

#[test]
#[function_name::named]
fn delete() {
    let _guard = LOCK.run_concurrently();

    let coll = CLIENT.init_db_and_coll(function_name!(), function_name!());

    let result = coll
        .insert_many((0i32..5).map(|_| doc! { "x": 3 }).collect::<Vec<_>>(), None)
        .unwrap();
    assert_eq!(result.inserted_ids.len(), 5);

    let delete_one_result = coll.delete_one(doc! {"x": 3}, None).unwrap();
    assert_eq!(delete_one_result.deleted_count, 1);

    assert_eq!(coll.count_documents(doc! {"x": 3}, None).unwrap(), 4);
    let delete_many_result = coll.delete_many(doc! {"x": 3}, None).unwrap();
    assert_eq!(delete_many_result.deleted_count, 4);
    assert_eq!(coll.count_documents(doc! {"x": 3 }, None).unwrap(), 0);
}

#[test]
#[function_name::named]
fn aggregate_out() {
    let _guard = LOCK.run_concurrently();

    let db = CLIENT.database(function_name!());
    let coll = db.collection(function_name!());

    drop_collection(&coll);

    let result = coll
        .insert_many((0i32..5).map(|n| doc! { "x": n }).collect::<Vec<_>>(), None)
        .unwrap();
    assert_eq!(result.inserted_ids.len(), 5);

    let out_coll = db.collection(&format!("{}_1", function_name!()));
    let pipeline = vec![
        doc! {
            "$match": {
                "x": { "$gt": 1 },
            }
        },
        doc! {"$out": out_coll.name()},
    ];
    drop_collection(&out_coll);

    coll.aggregate(pipeline.clone(), None).unwrap();
    assert!(db
        .list_collection_names(None)
        .unwrap()
        .into_iter()
        .any(|name| name.as_str() == out_coll.name()));
    drop_collection(&out_coll);

    // check that even with a batch size of 0, a new collection is created.
    coll.aggregate(pipeline, AggregateOptions::builder().batch_size(0).build())
        .unwrap();
    assert!(db
        .list_collection_names(None)
        .unwrap()
        .into_iter()
        .any(|name| name.as_str() == out_coll.name()));
}

fn kill_cursors_sent(client: &EventClient) -> bool {
    client
        .command_events
        .read()
        .unwrap()
        .iter()
        .any(|event| match event {
            CommandEvent::CommandStartedEvent(CommandStartedEvent { command_name, .. }) => {
                command_name == "killCursors"
            }
            _ => false,
        })
}

#[test]
#[function_name::named]
fn kill_cursors_on_drop() {
    let _guard = LOCK.run_concurrently();

    let db = CLIENT.database(function_name!());
    let coll = db.collection(function_name!());

    drop_collection(&coll);

    coll.insert_many(vec![doc! { "x": 1 }, doc! { "x": 2 }], None)
        .unwrap();

    let event_client = EventClient::new();
    let coll = event_client
        .database(function_name!())
        .collection(function_name!());

    let cursor = coll
        .find(None, FindOptions::builder().batch_size(1).build())
        .unwrap();

    assert!(!kill_cursors_sent(&event_client));

    std::mem::drop(cursor);

    assert!(kill_cursors_sent(&event_client));
}

#[test]
#[function_name::named]
fn no_kill_cursors_on_exhausted() {
    let _guard = LOCK.run_concurrently();

    let db = CLIENT.database(function_name!());
    let coll = db.collection(function_name!());

    drop_collection(&coll);

    coll.insert_many(vec![doc! { "x": 1 }, doc! { "x": 2 }], None)
        .unwrap();

    let event_client = EventClient::new();
    let coll = event_client
        .database(function_name!())
        .collection(function_name!());

    let cursor = coll.find(None, FindOptions::builder().build()).unwrap();

    assert!(!kill_cursors_sent(&event_client));

    std::mem::drop(cursor);

    assert!(!kill_cursors_sent(&event_client));
}

lazy_static! {
    #[allow(clippy::unreadable_literal)]
    static ref LARGE_DOC: Document = doc! {
        "text": "the quick brown fox jumped over the lazy sheep dog",
        "in_reply_to_status_id": 22213321312i64,
        "retweet_count": Bson::Null,
        "contributors": Bson::Null,
        "created_at": ";lkasdf;lkasdfl;kasdfl;kasdkl;ffasdkl;fsadkl;fsad;lfk",
        "geo": Bson::Null,
        "source": "web",
        "coordinates": Bson::Null,
        "in_reply_to_screen_name": "sdafsdafsdaffsdafasdfasdfasdfasdfsadf",
        "truncated": false,
        "entities": {
            "user_mentions": [
                {
                    "indices": [
                        0,
                        9
                    ],
                    "screen_name": "sdafsdaff",
                    "name": "sadfsdaffsadf",
                    "id": 1
                }
            ],
            "urls": [],
            "hashtags": []
        },
        "retweeted": false,
        "place": Bson::Null,
        "user": {
            "friends_count": 123,
            "profile_sidebar_fill_color": "7a7a7a",
            "location": "sdafffsadfsdaf sdaf asdf asdf sdfsdfsdafsdaf asdfff sadf",
            "verified": false,
            "follow_request_sent": Bson::Null,
            "favourites_count": 0,
            "profile_sidebar_border_color": "a3a3a3",
            "profile_image_url": "sadfkljajsdlkffajlksdfjklasdfjlkasdljkf asdjklffjlksadfjlksadfjlksdafjlksdaf",
            "geo_enabled": false,
            "created_at": "sadffasdffsdaf",
            "description": "sdffasdfasdfasdfasdf",
            "time_zone": "Mountain Time (US & Canada)",
            "url": "sadfsdafsadf fsdaljk asjdklf lkjasdf lksadklfffsjdklafjlksdfljksadfjlk",
            "screen_name": "adsfffasdf sdlk;fjjdsakfasljkddfjklasdflkjasdlkfjldskjafjlksadf",
            "notifications": Bson::Null,
            "profile_background_color": "303030",
            "listed_count": 1,
            "lang": "en"
        }
    };
}

#[test]
#[function_name::named]
fn large_insert() {
    let _guard = LOCK.run_concurrently();

    let docs = vec![LARGE_DOC.clone(); 35000];

    let coll = CLIENT.init_db_and_coll(function_name!(), function_name!());
    assert_eq!(
        coll.insert_many(docs, None).unwrap().inserted_ids.len(),
        35000
    );
}

/// Returns a vector of documents that cannot be sent in one batch (35000 documents).
/// Includes duplicate _id's across different batches.
fn multibatch_documents_with_duplicate_keys() -> Vec<Document> {
    let large_doc = LARGE_DOC.clone();

    let mut docs: Vec<Document> = Vec::new();
    docs.extend(vec![large_doc.clone(); 7498]);

    docs.push(doc! { "_id": 1 });
    docs.push(doc! { "_id": 1 }); // error in first batch, index 7499

    docs.extend(vec![large_doc.clone(); 14999]);
    docs.push(doc! { "_id": 1 }); // error in second batch, index 22499

    docs.extend(vec![large_doc.clone(); 9999]);
    docs.push(doc! { "_id": 1 }); // error in third batch, index 32499

    docs.extend(vec![large_doc; 2500]);

    assert_eq!(docs.len(), 35000);
    docs
}

#[test]
#[function_name::named]
fn large_insert_unordered_with_errors() {
    let _guard = LOCK.run_concurrently();

    let docs = multibatch_documents_with_duplicate_keys();

    let coll = CLIENT.init_db_and_coll(function_name!(), function_name!());
    let options = InsertManyOptions::builder().ordered(false).build();

    match coll
        .insert_many(docs, options)
        .expect_err("should get error")
        .kind
        .as_ref()
    {
        ErrorKind::BulkWriteError(ref failure) => {
            let mut write_errors = failure
                .write_errors
                .clone()
                .expect("should have write errors");
            assert_eq!(write_errors.len(), 3);
            write_errors.sort_by(|lhs, rhs| lhs.index.cmp(&rhs.index));

            assert_eq!(write_errors[0].index, 7499);
            assert_eq!(write_errors[1].index, 22499);
            assert_eq!(write_errors[2].index, 32499);
        }
        e => panic!("expected bulk write error, got {:?} instead", e),
    }
}

#[test]
#[function_name::named]
fn large_insert_ordered_with_errors() {
    let _guard = LOCK.run_concurrently();

    let docs = multibatch_documents_with_duplicate_keys();

    let coll = CLIENT.init_db_and_coll(function_name!(), function_name!());
    let options = InsertManyOptions::builder().ordered(true).build();

    match coll
        .insert_many(docs, options)
        .expect_err("should get error")
        .kind
        .as_ref()
    {
        ErrorKind::BulkWriteError(ref failure) => {
            let write_errors = failure
                .write_errors
                .clone()
                .expect("should have write errors");
            assert_eq!(write_errors.len(), 1);
            assert_eq!(write_errors[0].index, 7499);
            assert_eq!(
                coll.count_documents(None, None)
                    .expect("count should succeed"),
                7499
            );
        }
        e => panic!("expected bulk write error, got {:?} instead", e),
    }
}

#[test]
#[function_name::named]
fn empty_insert() {
    let _guard = LOCK.run_concurrently();

    let coll = CLIENT
        .database(function_name!())
        .collection(function_name!());
    match coll
        .insert_many(Vec::new(), None)
        .expect_err("should get error")
        .kind
        .as_ref()
    {
        ErrorKind::ArgumentError { .. } => {}
        e => panic!("expected argument error, got {:?}", e),
    };
}
