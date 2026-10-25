//! The part of the data dictionary (K3) the sample module declares: its note, each field with its class, once. The
//! kernel validates a note with it, lists what is missing and knows that the phone is protected.

use kernel::{Entity, Field, Kind};

pub const NOTE: Entity = Entity {
    id: "note",
    fields: &[
        Field::new("title", Kind::Text, "internal").mandatory().max_len(120),
        Field::new("body", Kind::LongText, "internal"),
        Field::new("phone", Kind::Phone, "personal"),
    ],
};
