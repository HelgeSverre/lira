# test_json

JSON Module Tests
@expect-contains: parse int: 42
@expect-contains: parse string: hello
@expect-contains: parse bool: true
@expect-contains: parse null: null
@expect-contains: parse array length: 3
@expect-contains: parse object name: John
@expect-contains: stringify object
@expect-contains: roundtrip passed
@expect-contains: nested access: world
