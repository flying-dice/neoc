Feature: Emitting Python

  The Python backend (target #5) lowers a neoc module to idiomatic Python 3.10+
  using native constructs: a `struct` becomes a frozen `@dataclass`, an `enum`
  becomes one `@dataclass` per variant plus a `Union` alias, `match` lowers to
  Python's native structural `match`/`case`, and `Result`/`Option` are tagged
  dataclasses imported from the shared `neoc_lib`. `number` is a double, so it
  maps to `float`; `string` → `str`; `()` → `None`; `[T]` → `list[T]`; a
  closure type → `Callable[[A], B]`. A sibling `.pyi` carries the typed surface.
  The shapes asserted here mirror the crate goldens and `lang/DYNAMIC.md`.

  Scenario: A struct lowers to a frozen dataclass with float fields
    Given the source:
      """
      pub struct Row { id: number, body: string }
      """
    When I emit Python
    Then the Python contains "@dataclass(frozen=True)"
    And the Python contains "class Row:"
    And the Python contains "    id: float"
    And the Python contains "    body: str"
    And the .pyi contains "class Row:"
    And the .pyi contains "    id: float"
    And the emit reports no problems

  Scenario: An enum lowers to a dataclass per variant plus a union alias
    Given the source:
      """
      pub enum E { A, B(number), C { x: string } }
      """
    When I emit Python
    Then the Python contains "class A:"
    And the Python contains "class B:"
    And the Python contains "    _0: float"
    And the Python contains "class C:"
    And the Python contains "    x: str"
    And the Python contains "E = Union[A, B, C]"

  Scenario: A pub fn returning Result carries its annotations
    Given the source:
      """
      pub fn find(id: number) -> Result<string, string> {
          Ok("x")
      }
      """
    When I emit Python
    Then the Python contains "def find(id: float) -> Result[str, str]:"
    And the Python contains "from .neoc_lib import Result, Option, Ok, Err, Some, None_"
    And the Python contains "return Ok("x")"
    And the .pyi contains "def find(id: float) -> Result[str, str]: ..."

  Scenario: A match lowers to native case patterns
    Given the source:
      """
      pub fn unwrap_or_zero(o: Option<number>) -> number {
          match o {
              Some(x) => x,
              None => 0,
          }
      }
      """
    When I emit Python
    Then the Python contains "match o:"
    And the Python contains "case Some(x):"
    And the Python contains "case None_():"

  Scenario: A method lands in the dataclass body
    Given the source:
      """
      pub struct Point { x: number, y: number }
      impl Point {
          fn sum(self) -> number { self.x + self.y }
      }
      """
    When I emit Python
    Then the Python contains "class Point:"
    And the Python contains "    def sum(self) -> float:"
    And the Python contains "return (self.x + self.y)"

  Scenario: The question-mark operator hoists a carrier guard
    Given the source:
      """
      pub fn chain(r: Result<number, string>) -> Result<number, string> {
          let x = r?;
          Ok(x)
      }
      """
    When I emit Python
    Then the Python contains "if isinstance("
    And the Python contains "(Err, None_)"

  Scenario: A use lowers to a relative import
    Given the source:
      """
      use math::add;
      pub fn f() -> number { add(1, 2) }
      """
    When I emit Python
    Then the Python contains "from .math import add"

  Scenario: List and closure types project idiomatically
    Given the source:
      """
      pub fn apply(xs: [number], g: fn(number) -> number) -> number { xs[0] }
      """
    When I emit Python
    Then the Python contains "def apply(xs: list[float], g: Callable[[float], float]) -> float:"
