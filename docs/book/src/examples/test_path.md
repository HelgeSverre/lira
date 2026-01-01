# test_path

Path Module Tests
Tests for the std.path module
@expect-contains: dirname: /foo/bar
@expect-contains: basename: file.txt
@expect-contains: extension: .txt
@expect-contains: stem: file
@expect-contains: is_absolute: true
@expect-contains: is_relative: true
@expect-contains: normalize test passed
@expect-contains: join test passed
@expect-contains: with_extension test passed
@expect-contains: components test passed
@expect-contains: All Path Tests Passed
