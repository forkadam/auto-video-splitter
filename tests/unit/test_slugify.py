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


def test_empty_result_falls_back_to_unnamed():
    assert slugify("<>:.mp4") == "_unnamed"
    assert slugify("..mp4") == "_unnamed"
    assert slugify("   .mp4") == "_unnamed"


def test_control_chars_are_stripped():
    assert slugify("a\x00b.mp4") == "ab"
    assert slugify("\x01\x02hello.mp4") == "hello"
    assert slugify("tab\there.mp4") == "tab-here"  # \t is whitespace, replaced not stripped


def test_truncation_does_not_leave_trailing_hyphen():
    name = "a" * 63 + "-rest.mp4"
    s = slugify(name)
    assert len(s) <= 64
    assert not s.endswith("-")


def test_resolve_collision_respects_max_length():
    long_slug = "a" * 64
    existing = {long_slug}
    result = resolve_collision(long_slug, existing, set())
    assert len(result) <= 64
    assert result != long_slug


def test_resolve_collision_raises_when_exhausted():
    import pytest as _pytest
    base = "x"
    taken = {base, *(f"{base}-{n}" for n in range(2, 100))}
    with _pytest.raises(ValueError, match="more than 99 slug collisions"):
        resolve_collision(base, taken, set())
