from vsplit.core.slugify import slugify, resolve_collision

def test_basic_lowercase():
    assert slugify("Birthday.mov") == "birthday"

def test_whitespace_and_underscore_to_hyphen():
    assert slugify("Beach Day_2024.mp4") == "beach-day-2024"

def test_strips_reserved_chars():
    assert slugify('Movie<>:"|?*.mkv') == "movie"

def test_strips_trailing_dots_and_spaces():
    assert slugify("Foo.   .mkv") == "foo"

def test_reserved_name_prefixed():
    assert slugify("CON.mp4") == "_con"
    assert slugify("nul.mkv") == "_nul"
    assert slugify("COM1.mov") == "_com1"
    assert slugify("LPT9.avi") == "_lpt9"

def test_collapse_repeated_hyphens():
    assert slugify("a -- b -- c.mp4") == "a-b-c"

def test_truncate_to_64():
    long = "a" * 100 + ".mp4"
    s = slugify(long)
    assert len(s) <= 64

def test_resolve_collision_appends_suffix():
    existing = {"birthday"}
    retired = set()
    assert resolve_collision("birthday", existing, retired) == "birthday-2"

def test_resolve_collision_case_insensitive():
    existing = {"BIRTHDAY"}
    retired = set()
    assert resolve_collision("birthday", existing, retired) == "birthday-2"

def test_resolve_collision_skips_retired():
    existing = set()
    retired = {"birthday"}
    assert resolve_collision("birthday", existing, retired) == "birthday-2"
