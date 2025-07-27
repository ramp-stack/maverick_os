struct Message {
    id: Id,
    author: String,
    body: String
}

    Messages
 id | author | body    | _body_state
------------------------------------
 3  | bob    | "hello" | (17, 20)


--------------------------------------

struct TagGroup {
    group_name: String,
    tags: Map<String, Tag>
}

struct Tag {
    value: String,
    index: u32
}
