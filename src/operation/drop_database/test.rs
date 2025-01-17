use bson::doc;

use crate::{
    cmap::{CommandResponse, StreamDescription},
    concern::{Acknowledgment, WriteConcern},
    error::{ErrorKind, WriteFailure},
    operation::{DropDatabase, Operation},
    options::DropDatabaseOptions,
};

#[test]
fn build() {
    let op = DropDatabase {
        target_db: "test_db".to_string(),
        options: Some(DropDatabaseOptions {
            write_concern: Some(WriteConcern {
                w: Some(Acknowledgment::Tag("abc".to_string())),
                ..Default::default()
            }),
        }),
    };

    let description = StreamDescription::new_testing();
    let cmd = op.build(&description).expect("build should succeed");

    assert_eq!(cmd.name.as_str(), "dropDatabase");
    assert_eq!(cmd.target_db.as_str(), "test_db");
    assert_eq!(cmd.read_pref.as_ref(), None);
    assert_eq!(
        cmd.body,
        doc! {
            "dropDatabase": 1,
            "writeConcern": { "w": "abc" }
        }
    );

    let op = DropDatabase {
        target_db: "test_db".to_string(),
        options: None,
    };
    let cmd = op.build(&description).expect("build should succeed");
    assert_eq!(cmd.name.as_str(), "dropDatabase");
    assert_eq!(cmd.target_db.as_str(), "test_db");
    assert_eq!(cmd.read_pref.as_ref(), None);
    assert_eq!(
        cmd.body,
        doc! {
            "dropDatabase": 1,
        }
    );
}

#[test]
fn handle_success() {
    let op = DropDatabase::empty();

    let ok_response = CommandResponse::with_document(doc! { "ok": 1.0 });
    assert!(op.handle_response(ok_response).is_ok());
    let ok_extra = CommandResponse::with_document(doc! { "ok": 1.0, "hello": "world" });
    assert!(op.handle_response(ok_extra).is_ok());
}

#[test]
fn handle_write_concern_error() {
    let op = DropDatabase::empty();

    let response = CommandResponse::with_document(doc! {
        "writeConcernError": {
            "code": 100,
            "codeName": "hello world",
            "errmsg": "12345"
        },
        "ok": 1
    });

    let result = op.handle_response(response);
    assert!(result.is_err());

    match *result.unwrap_err().kind {
        ErrorKind::WriteError(WriteFailure::WriteConcernError(ref wc_err)) => {
            assert_eq!(wc_err.code, 100);
            assert_eq!(wc_err.code_name, "hello world");
            assert_eq!(wc_err.message, "12345");
        }
        ref e => panic!("expected write concern error, got {:?}", e),
    }
}
