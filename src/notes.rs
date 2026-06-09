ChatRoom Id = abc_chat_room;

Data:
    abc_chat_room:
        instance_0:
            name: my_room,
            author: Caleb,
            messages:
                0:
                    author: Bob
                    timestamp: 4958383,
                    body: "This is the first message"
                    comments:
                        0:
                            author: Alice
                            timestamp: 5995929,
                            body: "This was a dumb first Message Bob"


Contract ChatRoom:
    routes: 
        "/name": [ChangeName]
        "/messages": [SendMessage]
        "/messages/*": [EditMessage]
        "/messages/*/comments": [SendComment]
    reactants:
        ChangeName(String):
            if /author == author { write self to /name }
        SendMessage(String):
            push Message{body: self, author, timestamp} to ./
        EditMessage(String):
            write self to ./body
        SendComment(String):
            push Comment{body: self, author, timestamp} to ./

1. Not all routes have their own channels
2. Not all channels have their own sockets
3. Not all contracts or instances have their own threads
4. Not all reactants have to be processed to get desired result
5. Not all parts of a reactant have to be run to get desired result
6. Not all parts of the instance has to be stored on disk or in memory
7. Cost evaluating contract language needs the context to include all possible reads?
8. Evaluating the cost of a partial run of a function might be a few levels too far in abstraction?
9. The ability to scope the run of the API is what prevents developers from having to write there own applicators.
10. Writing your own partial applicators are error prone and not provable
11. Channels are currently sequential but this is not a requirement, handshaking/partitioned channels exist
12. The ability to scope a contract is the same ability that allows me to have invite only channels full of reactants



InstanceData:
    value_a: 0,
    value_b: 0

Contract Counter:
    routes:
        "/": [AddOne , Multiply, Pow]
    reactants:
        AddOne:
            read /value_a,
            write /value_a += 1
            read /value_b,
            write /value_b += 1
      //Multiply:
      //    read /value_a,
      //    read /vaule_b
      //    write /value_b = value_a * value_b;
      //Pow(u32):
      //    read /value_a
      //    let temp;
      //    for i in self {
      //        temp = value_a * value_a
      //    }
      //    write temp to /value_a

Examples:
    Fetching a profile for a list of names each on different air servers
    Joining a room that spans three air servers
    Running a bitcoin service remotely or locally updating your balance from remote resources ex: blockstream.info
    Large file system like GDrive and editing a file(s) in realtime



Instance:
    author: Caleb,
    admins: [Bob, Alice, Charlie],
    system:
        my_root_file,
            data: "Bobs first file"
        my_root_folder:
            my_nested_folder:
                my_nested_nested_file
                    "Charlie Likes deep files"
            my_nested_file
                "Alice created this one,\n Then Bob Updated this One"
History:
    Caleb /admins: AddAdmin(Bob),
    Caleb /admins: AddAdmin(Alice),
    Caleb /admins: AddAdmin(Charlie),
    Bob /system: NewFile("my_root_file", "Bobs first file"),
    Caleb /system: NewFolder("my_root_folder"),
    Charlie /system/my_root_folder: NewFolder("my_nested_folder"),
    Charlie /system/my_root_folder/my_nested_folder: NewFile("my_nested_nested_file", "Charlie Likes deep files"),
    Alice: /system/my_root_folder: NewFile("my_nested_file", "Alice Created This One"),
    Bob: /system/my_root_folder/my_nested_file: Update(1..1, ",\n Then Bob Updated This One"),
    Caleb: /system: NewFile("file", "Going to be deleted next"),
    Caleb /system/file: Delete

Contract FileSystem:
    routes:
        "admins": [AddAdmin, RemoveAdmin]
        "system/*": [NewFolder, NewFile, Rename, Update, Delete]
    reactants:
        AddAdmin(Name):
            read /author = author {insert self into /admins}
        RemoveAdmin(Name):
            read /author = author {remove self from /admins}
        NewFolder(String):
            read ../@type = Folder {write Folder{} to ./self}
        NewFile(String, String):
            read ../@type = Folder {write File{data: self.data} to ./self.name}
        Update(u32..u32, String):
            read ./
            ensure its a file
            apply self as a diff to the data
        Delete:
            read ../@type = Folder
            read ./@exists
            write null to ./














shared_key: 48572885795

shared_key/0: 5868838282882

CreateFile(5868838282882, b"My byte string containing an reactant, or update")

shared_key/0: 5868838282882

ReadFile(5868838282882)

Subscribe(5868838282882, timeout_len)

shared_key/0: b"I am bob/device_id"
shared_key/bob/device_id/0: 4857429957285
shared_key/alice/0:


