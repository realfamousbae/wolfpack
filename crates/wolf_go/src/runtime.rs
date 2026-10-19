//! The Go runtime: the package `wolfrt`, generated into every output project (spec chapter 8,
//! section 2, rule 2).
//!
//! It also holds the declarations of std (`Ordering`, `Error`, `ParseError`, `Entry`, `FsError`
//! and the constants of `std.math`) and its intrinsic functions, so that std needs no packages of
//! its own: package names such as `map` and `string` are not valid in Go, and one package cannot
//! form import cycles. The runtime is split into pieces; only the pieces a program uses and their
//! dependencies are written.

use std::collections::BTreeSet;

/// A piece of the runtime.
pub struct Piece {
    /// The name the backend asks for.
    pub name: &'static str,
    /// Pieces this one uses.
    pub deps: &'static [&'static str],
    /// Standard packages this piece imports.
    pub imports: &'static [&'static str],
    /// The Go source, formatted as `gofmt` formats it.
    pub code: &'static str,
}

/// Every piece, in the order they are written.
pub const PIECES: &[Piece] = &[
    Piece {
        name: "Option",
        deps: &[],
        imports: &[],
        code: r#"// Option is an Option stored as data.
type Option[T any] struct {
	Some  bool
	Value T
}

// Some is Some(value).
func Some[T any](value T) Option[T] {
	return Option[T]{Some: true, Value: value}
}

// None is None.
func None[T any]() Option[T] {
	return Option[T]{}
}

// OptionOf turns the result of a function, (value, ok), into data.
func OptionOf[T any](value T, ok bool) Option[T] {
	return Option[T]{Some: ok, Value: value}
}

// OptionOfPtr turns the result of a function, a pointer or nil, into data.
func OptionOfPtr[T any](p *T) Option[T] {
	if p == nil {
		return Option[T]{}
	}
	return Option[T]{Some: true, Value: *p}
}

// Get is the option as the result of a function: (value, ok).
func (o Option[T]) Get() (T, bool) {
	return o.Value, o.Some
}

// Ptr is the option as the result of a function: a pointer or nil.
func (o Option[T]) Ptr() *T {
	if !o.Some {
		return nil
	}
	return &o.Value
}

// PtrOf turns (value, ok) into a pointer or nil.
func PtrOf[T any](value T, ok bool) *T {
	if !ok {
		return nil
	}
	return &value
}

// Ptr is a pointer to a copy of the value.
func Ptr[T any](value T) *T {
	return &value
}

// Deref turns a pointer or nil into (value, ok).
func Deref[T any](p *T) (T, bool) {
	if p == nil {
		var zero T
		return zero, false
	}
	return *p, true
}

// OptionEqual is == of two options.
func OptionEqual[T any](a, b Option[T], eq func(T, T) bool) bool {
	if a.Some != b.Some {
		return false
	}
	return !a.Some || eq(a.Value, b.Value)
}

// CopyOption is .copy() of an option.
func CopyOption[T any](o Option[T], copy func(T) T) Option[T] {
	if !o.Some {
		return o
	}
	return Option[T]{Some: true, Value: copy(o.Value)}
}"#,
    },
    Piece {
        name: "Result",
        deps: &[],
        imports: &[],
        code: r#"// Result is a Result stored as data.
type Result[T, E any] struct {
	Ok    bool
	Value T
	Err   E
}

// Ok is Ok(value).
func Ok[T, E any](value T) Result[T, E] {
	return Result[T, E]{Ok: true, Value: value}
}

// Err is Err(error).
func Err[T, E any](err E) Result[T, E] {
	return Result[T, E]{Err: err}
}

// ResultOf turns the result of a function, (value, err), into data.
func ResultOf[T, E any](value T, err error) Result[T, E] {
	if err != nil {
		return Result[T, E]{Err: err.(E)}
	}
	return Result[T, E]{Ok: true, Value: value}
}

// ResultOfErr turns the result of a function that returns only an error into data.
func ResultOfErr[E any](err error) Result[struct{}, E] {
	return ResultOf[struct{}, E](struct{}{}, err)
}

// Get is the result as the result of a function: (value, err).
func (r Result[T, E]) Get() (T, error) {
	if !r.Ok {
		return r.Value, any(r.Err).(error)
	}
	return r.Value, nil
}

// AsError is a Result[struct{}, E] as the result of a function: an error or nil.
func (r Result[T, E]) AsError() error {
	if !r.Ok {
		return any(r.Err).(error)
	}
	return nil
}

// ResultEqual is == of two results.
func ResultEqual[T, E any](a, b Result[T, E], eqT func(T, T) bool, eqE func(E, E) bool) bool {
	if a.Ok != b.Ok {
		return false
	}
	if a.Ok {
		return eqT(a.Value, b.Value)
	}
	return eqE(a.Err, b.Err)
}

// CopyResult is .copy() of a result.
func CopyResult[T, E any](r Result[T, E], copyT func(T) T, copyE func(E) E) Result[T, E] {
	if r.Ok {
		return Result[T, E]{Ok: true, Value: copyT(r.Value)}
	}
	return Result[T, E]{Err: copyE(r.Err)}
}"#,
    },
    Piece {
        name: "Eq",
        deps: &[],
        imports: &[],
        code: r#"// Eq is == of values that Go compares itself.
func Eq[T comparable](a, b T) bool {
	return a == b
}

// Same is .copy() of values that are copied by assignment.
func Same[T any](value T) T {
	return value
}"#,
    },
    Piece {
        name: "ListEqual",
        deps: &[],
        imports: &[],
        code: r#"// ListEqual is == of two lists.
func ListEqual[T any](a, b []T, eq func(T, T) bool) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if !eq(a[i], b[i]) {
			return false
		}
	}
	return true
}

// CopyList is .copy() of a list.
func CopyList[T any](items []T, copy func(T) T) []T {
	out := make([]T, len(items))
	for i, item := range items {
		out[i] = copy(item)
	}
	return out
}"#,
    },
    Piece {
        name: "Ordering",
        deps: &[],
        imports: &[],
        code: r#"// Ordering is the result of compare.
type Ordering int

const (
	OrderingLess Ordering = iota
	OrderingEqual
	OrderingGreater
)

// CompareInt is Int.compare.
func CompareInt(a, b int64) Ordering {
	switch {
	case a < b:
		return OrderingLess
	case a > b:
		return OrderingGreater
	}
	return OrderingEqual
}

// CompareString is String.compare: by Unicode scalar values, which is the order of the bytes
// of UTF-8.
func CompareString(a, b string) Ordering {
	switch {
	case a < b:
		return OrderingLess
	case a > b:
		return OrderingGreater
	}
	return OrderingEqual
}

func order(o Ordering) int {
	switch o {
	case OrderingLess:
		return -1
	case OrderingGreater:
		return 1
	}
	return 0
}"#,
    },
    Piece {
        name: "Is",
        deps: &[],
        imports: &[],
        code: r#"// Is reports whether x holds a value of type V.
func Is[V any](x any) bool {
	_, ok := x.(V)
	return ok
}"#,
    },
    Piece {
        name: "Error",
        deps: &[],
        imports: &[],
        code: r#"// Error is an error that can be shown to a person.
type Error interface {
	error
	Message() string
}"#,
    },
    Piece {
        name: "TextError",
        deps: &["Error"],
        imports: &[],
        code: r#"// TextError is the error of error(message).
type TextError struct {
	text string
}

// NewTextError is error(message): an Error with this message.
func NewTextError(text string) Error {
	return TextError{text: text}
}

// Message is the text of the error.
func (e TextError) Message() string {
	return e.text
}

func (e TextError) Error() string {
	return e.text
}"#,
    },
    Piece {
        name: "Panic",
        deps: &[],
        imports: &[],
        code: r#"// Panic is the value of a panic of a wolfpack program: its whole message.
type Panic struct {
	Message string
}

// Fail is a panic raised by the language itself, without a location.
func Fail(message string) {
	panic(Panic{Message: "panic: " + message})
}

// PanicAt is the value of panic(message) at a source location.
func PanicAt(message, location string) Panic {
	return Panic{Message: "panic at " + location + ": " + message}
}

// Assert is assert(condition): a panic at the location of the call.
func Assert(condition bool, text, location string) {
	if !condition {
		panic(Panic{Message: "panic at " + location + ": assertion failed: " + text})
	}
}"#,
    },
    Piece {
        name: "message",
        deps: &["Panic"],
        imports: &["fmt", "runtime"],
        code: r#"// message is the canonical message of a recovered panic (spec chapter 7, section 3).
func message(r any) string {
	switch v := r.(type) {
	case Panic:
		return v.Message
	case runtime.Error:
		if v.Error() == "runtime error: integer divide by zero" {
			return "panic: division by zero"
		}
		return "panic: " + v.Error()
	case error:
		return "panic: " + v.Error()
	}
	return "panic: " + fmt.Sprint(r)
}"#,
    },
    Piece {
        name: "RunMain",
        deps: &["message"],
        imports: &["os"],
        code: r#"func recovered() {
	if r := recover(); r != nil {
		os.Stderr.WriteString(message(r) + "\n")
		os.Exit(101)
	}
}

// RunMain runs main.
func RunMain(main func()) {
	defer recovered()
	main()
}"#,
    },
    Piece {
        name: "RunMainResult",
        deps: &["RunMain"],
        imports: &["os"],
        code: r#"// RunMainResult runs a main that returns Result<Void, Error>.
func RunMainResult(main func() error) {
	defer recovered()
	if err := main(); err != nil {
		text := err.Error()
		if e, ok := err.(interface{ Message() string }); ok {
			text = e.Message()
		}
		os.Stderr.WriteString("error: " + text + "\n")
		os.Exit(1)
	}
}"#,
    },
    Piece {
        name: "RunMainArgs",
        deps: &["RunMain", "process"],
        imports: &[],
        code: r#"// RunMainArgs runs a main that takes the command-line arguments.
func RunMainArgs(main func([]string)) {
	RunMain(func() { main(Args()) })
}"#,
    },
    Piece {
        name: "RunMainArgsResult",
        deps: &["RunMainResult", "process"],
        imports: &[],
        code: r#"// RunMainArgsResult runs a main that takes the command-line arguments and returns
// Result<Void, Error>.
func RunMainArgsResult(main func([]string) error) {
	RunMainResult(func() error { return main(Args()) })
}"#,
    },
    Piece {
        name: "Test",
        deps: &["message"],
        imports: &["fmt"],
        code: r#"// T is the part of *testing.T that tests use.
type T interface {
	FailNow()
}

// Test runs the body of a test; a panic fails the test with its message.
func Test(t T, body func()) {
	failed := true
	defer func() {
		if r := recover(); r != nil || failed {
			text := "test failed"
			if r != nil {
				text = message(r)
			}
			fmt.Println("--- wolf failure: " + text)
			t.FailNow()
		}
	}()
	body()
	failed = false
}"#,
    },
    Piece {
        name: "Exit",
        deps: &["Panic"],
        imports: &["os", "strconv"],
        code: r#"// Exit is std.process.exit.
func Exit(code int64) {
	if code < 0 || code > 255 {
		Fail("exit code " + strconv.FormatInt(code, 10) + " out of range")
	}
	os.Exit(int(code))
}"#,
    },
    Piece {
        name: "extern",
        deps: &["Panic"],
        imports: &["math"],
        code: r#"// FromTargetInt is an integer returned by extern code as an Int.
func FromTargetInt[T ~int | ~int8 | ~int16 | ~int32 | ~int64 | ~uint | ~uint8 | ~uint16 | ~uint32 | ~uint64 | ~uintptr](value T) int64 {
	if value > 0 && uint64(value) > math.MaxInt64 {
		Fail("integer overflow")
	}
	return int64(value)
}"#,
    },
    Piece {
        name: "externMap",
        deps: &["Map"],
        imports: &["cmp", "sort"],
        code: r#"// Same is a value that crosses the extern boundary as it is.
func Same[T any](value T) T {
	return value
}

// ToExternMap is a Map passed to extern code: a Go map with its keys and values converted.
func ToExternMap[K comparable, V any, K2 comparable, V2 any](m Map[K, V], key func(K) K2, value func(V) V2) map[K2]V2 {
	out := make(map[K2]V2, len(m.keys))
	for i, k := range m.keys {
		out[key(k)] = value(m.values[i])
	}
	return out
}

// FromExternMap is a map returned by extern code: converted and sorted by key.
func FromExternMap[K2 comparable, V2 any, K comparable, V any](m map[K2]V2, key func(K2) K, value func(V2) V, compare func(a, b K) int) Map[K, V] {
	keys := make([]K, 0, len(m))
	values := make(map[K]V, len(m))
	for k, v := range m {
		c := key(k)
		keys = append(keys, c)
		values[c] = value(v)
	}
	sort.Slice(keys, func(i, j int) bool { return compare(keys[i], keys[j]) < 0 })
	var out Map[K, V]
	for _, k := range keys {
		out.Insert(k, values[k])
	}
	return out
}

// ConvertList converts each element of a list at the extern boundary.
func ConvertList[T any, U any](list []T, f func(T) U) []U {
	out := make([]U, len(list))
	for i, x := range list {
		out[i] = f(x)
	}
	return out
}

// CompareKey orders Int and String keys of maps from extern code.
func CompareKey[K int64 | string](a, b K) int {
	return cmp.Compare(a, b)
}

// CompareBool orders Bool keys of maps from extern code: false first.
func CompareBool(a, b bool) int {
	switch {
	case a == b:
		return 0
	case a:
		return 1
	default:
		return -1
	}
}"#,
    },
    Piece {
        name: "externError",
        deps: &["Error", "TextError"],
        imports: &[],
        code: r#"// FromExternError is an error of extern code as an Error whose message is its text.
func FromExternError(err error) error {
	if err == nil {
		return nil
	}
	if e, ok := err.(Error); ok {
		return e
	}
	return TextError{text: err.Error()}
}"#,
    },
    Piece {
        name: "int",
        deps: &["Panic"],
        imports: &["math"],
        code: r#"// Add is a + b with an overflow check.
func Add(a, b int64) int64 {
	c := a + b
	if (a^c)&(b^c) < 0 {
		Fail("integer overflow")
	}
	return c
}

// Sub is a - b with an overflow check.
func Sub(a, b int64) int64 {
	c := a - b
	if (a^b)&(a^c) < 0 {
		Fail("integer overflow")
	}
	return c
}

// Mul is a * b with an overflow check.
func Mul(a, b int64) int64 {
	if a == 0 || b == 0 {
		return 0
	}
	c := a * b
	if c/b != a || (a == -1 && b == math.MinInt64) || (b == -1 && a == math.MinInt64) {
		Fail("integer overflow")
	}
	return c
}

// Neg is -a with an overflow check.
func Neg(a int64) int64 {
	if a == math.MinInt64 {
		Fail("integer overflow")
	}
	return -a
}

// Div is a / b, truncating toward zero.
func Div(a, b int64) int64 {
	if b == 0 {
		Fail("division by zero")
	}
	if b == -1 && a == math.MinInt64 {
		Fail("integer overflow")
	}
	return a / b
}

// Rem is a % b, with the sign of the dividend.
func Rem(a, b int64) int64 {
	if b == 0 {
		Fail("division by zero")
	}
	if b == -1 && a == math.MinInt64 {
		Fail("integer overflow")
	}
	return a % b
}

// Shl is a << n; the bits shifted out are discarded.
func Shl(a, n int64) int64 {
	if n < 0 || n > 63 {
		Fail("shift amount out of range")
	}
	return a << uint(n)
}

// Shr is a >> n, an arithmetic shift.
func Shr(a, n int64) int64 {
	if n < 0 || n > 63 {
		Fail("shift amount out of range")
	}
	return a >> uint(n)
}

// IntAbs is Int.abs.
func IntAbs(a int64) int64 {
	if a < 0 {
		return Neg(a)
	}
	return a
}

// IntPow is Int.pow.
func IntPow(a, exp int64) int64 {
	if exp < 0 {
		Fail("negative exponent")
	}
	result := int64(1)
	for i := int64(0); i < exp; i++ {
		result = Mul(result, a)
	}
	return result
}"#,
    },
    Piece {
        name: "index",
        deps: &["Panic"],
        imports: &["strconv"],
        code: r#"func index(i int64, length int) int {
	if i < 0 || i >= int64(length) {
		Fail("index " + strconv.FormatInt(i, 10) + " out of range for length " + strconv.Itoa(length))
	}
	return int(i)
}

// At is items[i].
func At[T any](items []T, i int64) T {
	return items[index(i, len(items))]
}

// SetAt is items[i] = value.
func SetAt[T any](items []T, i int64, value T) {
	items[index(i, len(items))] = value
}

// RefAt is items[i] as a place for a mut parameter.
func RefAt[T any](items []T, i int64) *T {
	return &items[index(i, len(items))]
}"#,
    },
    Piece {
        name: "bounds",
        deps: &["Panic"],
        imports: &["strconv"],
        code: r#"func bounds(start, end int64, length int) (int, int) {
	if start < 0 || end > int64(length) || start > end {
		Fail("slice " + strconv.FormatInt(start, 10) + ".." + strconv.FormatInt(end, 10) + " out of range for length " + strconv.Itoa(length))
	}
	return int(start), int(end)
}"#,
    },
    Piece {
        name: "SliceList",
        deps: &["bounds"],
        imports: &[],
        code: r#"// SliceList is items[start..end]: a new list of copies.
func SliceList[T any](items []T, start, end int64, copy func(T) T) []T {
	a, b := bounds(start, end, len(items))
	out := make([]T, 0, b-a)
	for _, item := range items[a:b] {
		out = append(out, copy(item))
	}
	return out
}

// SliceListFrom is items[start..].
func SliceListFrom[T any](items []T, start int64, copy func(T) T) []T {
	return SliceList(items, start, int64(len(items)), copy)
}"#,
    },
    Piece {
        name: "SliceStr",
        deps: &["bounds", "StrLen"],
        imports: &[],
        code: r#"// SliceStr is text[start..end], counting Unicode scalar values.
func SliceStr(text string, start, end int64) string {
	chars := []rune(text)
	a, b := bounds(start, end, len(chars))
	return string(chars[a:b])
}

// SliceStrFrom is text[start..].
func SliceStrFrom(text string, start int64) string {
	return SliceStr(text, start, StrLen(text))
}"#,
    },
    Piece {
        name: "StrLen",
        deps: &[],
        imports: &["unicode/utf8"],
        code: r#"// StrLen is the length in Unicode scalar values.
func StrLen(text string) int64 {
	return int64(utf8.RuneCountInString(text))
}"#,
    },
    Piece {
        name: "Display",
        deps: &["FloatString"],
        imports: &["strconv"],
        code: r#"// Display is a value that can be shown as text.
type Display interface {
	ToString() string
}

// IntString is to_string of Int.
func IntString(x int64) string {
	return strconv.FormatInt(x, 10)
}

// StringString is to_string of String.
func StringString(x string) string {
	return x
}

// ShowInt is an Int used as a Display value.
type ShowInt int64

// ToString shows the Int.
func (x ShowInt) ToString() string {
	return strconv.FormatInt(int64(x), 10)
}

// ShowFloat is a Float used as a Display value.
type ShowFloat float64

// ToString shows the Float.
func (x ShowFloat) ToString() string {
	return FloatString(float64(x))
}

// ShowBool is a Bool used as a Display value.
type ShowBool bool

// ToString shows the Bool.
func (x ShowBool) ToString() string {
	return strconv.FormatBool(bool(x))
}

// ShowString is a String used as a Display value.
type ShowString string

// ToString shows the String.
func (x ShowString) ToString() string {
	return string(x)
}"#,
    },
    Piece {
        name: "FloatString",
        deps: &[],
        imports: &["math", "strconv", "strings"],
        code: r#"// FloatString is the canonical format of Float (spec chapter 7, section 4.1), which is the
// format of JavaScript with "inf" and a ".0" for whole numbers.
func FloatString(x float64) string {
	switch {
	case math.IsNaN(x):
		return "NaN"
	case math.IsInf(x, 1):
		return "inf"
	case math.IsInf(x, -1):
		return "-inf"
	case x == 0:
		if math.Signbit(x) {
			return "-0.0"
		}
		return "0.0"
	}
	sign := ""
	if x < 0 {
		sign = "-"
		x = -x
	}
	e := strconv.FormatFloat(x, 'e', -1, 64)
	mantissa, exp, _ := strings.Cut(e, "e")
	digits := strings.Replace(mantissa, ".", "", 1)
	n, _ := strconv.Atoi(exp)
	n++
	k := len(digits)
	var text string
	switch {
	case k <= n && n <= 21:
		text = digits + strings.Repeat("0", n-k) + ".0"
	case 0 < n && n <= 21:
		text = digits[:n] + "." + digits[n:]
	case -6 < n && n <= 0:
		text = "0." + strings.Repeat("0", -n) + digits
	default:
		text = digits[:1]
		if k > 1 {
			text += "." + digits[1:]
		}
		text += "e" + strconv.Itoa(n-1)
	}
	return sign + text
}"#,
    },
    Piece {
        name: "float",
        deps: &["Panic"],
        imports: &["math"],
        code: r#"// FloatToInt is Float.to_int.
func FloatToInt(x float64) int64 {
	if math.IsNaN(x) || math.IsInf(x, 0) {
		Fail("float to int conversion of a non-finite value")
	}
	t := math.Trunc(x)
	if t < -9223372036854775808.0 || t >= 9223372036854775808.0 {
		Fail("integer overflow")
	}
	return int64(t)
}

// FloatAdd is a + b for constants, which Go would add exactly at compile time.
func FloatAdd(a, b float64) float64 {
	return a + b
}

// FloatSub is a - b for constants.
func FloatSub(a, b float64) float64 {
	return a - b
}

// FloatMul is a * b for constants.
func FloatMul(a, b float64) float64 {
	return a * b
}

// FloatDiv is a / b for constants.
func FloatDiv(a, b float64) float64 {
	return a / b
}

// FloatMin is Float.min: NaN is ignored.
func FloatMin(a, b float64) float64 {
	switch {
	case math.IsNaN(a):
		return b
	case math.IsNaN(b):
		return a
	}
	return math.Min(a, b)
}

// FloatMax is Float.max: NaN is ignored.
func FloatMax(a, b float64) float64 {
	switch {
	case math.IsNaN(a):
		return b
	case math.IsNaN(b):
		return a
	}
	return math.Max(a, b)
}"#,
    },
    Piece {
        name: "math",
        deps: &[],
        imports: &[],
        code: r#"// Pi is std.math.pi.
const Pi float64 = 3.141592653589793

// E is std.math.e.
const E float64 = 2.718281828459045

// IntMin is std.math.int_min.
const IntMin int64 = -9223372036854775808

// IntMax is std.math.int_max.
const IntMax int64 = 9223372036854775807"#,
    },
    Piece {
        name: "print",
        deps: &[],
        imports: &["os"],
        code: r#"// Print is std.io.print.
func Print(text string) {
	os.Stdout.WriteString(text + "\n")
}

// Write is std.io.write.
func Write(text string) {
	os.Stdout.WriteString(text)
}

// Eprint is std.io.eprint.
func Eprint(text string) {
	os.Stderr.WriteString(text + "\n")
}"#,
    },
    Piece {
        name: "ReadLine",
        deps: &[],
        imports: &["bufio", "os", "strings"],
        code: r#"var input *bufio.Reader

// ReadLine is std.io.read_line: a line without its terminator; false at the end of input.
func ReadLine() (string, bool) {
	if input == nil {
		input = bufio.NewReader(os.Stdin)
	}
	line, err := input.ReadString('\n')
	if err != nil && line == "" {
		return "", false
	}
	line = strings.TrimSuffix(line, "\n")
	return strings.TrimSuffix(line, "\r"), true
}"#,
    },
    Piece {
        name: "process",
        deps: &[],
        imports: &["os"],
        code: r#"// Args is std.io.args.
func Args() []string {
	return append([]string{}, os.Args[1:]...)
}

// Env is std.process.env.
func Env(name string) (string, bool) {
	return os.LookupEnv(name)
}"#,
    },
    Piece {
        name: "Replace",
        deps: &[],
        imports: &[],
        code: r#"// Replace is replace(mut place, own value): it stores the value and returns the old one.
func Replace[T any](place *T, value T) T {
	old := *place
	*place = value
	return old
}"#,
    },
    Piece {
        name: "strings",
        deps: &["Panic", "StrLen"],
        imports: &["strings"],
        code: r#"// StrFind is String.find: the position of the first occurrence, in scalar values.
func StrFind(text, part string) (int64, bool) {
	i := strings.Index(text, part)
	if i < 0 {
		return 0, false
	}
	return StrLen(text[:i]), true
}

// StrSplit is String.split.
func StrSplit(text, separator string) []string {
	if separator == "" {
		Fail("empty separator")
	}
	return strings.Split(text, separator)
}

// StrLines is String.lines.
func StrLines(text string) []string {
	lines := strings.Split(text, "\n")
	for i, line := range lines {
		lines[i] = strings.TrimSuffix(line, "\r")
	}
	if lines[len(lines)-1] == "" {
		lines = lines[:len(lines)-1]
	}
	return lines
}

// StrChars is String.chars.
func StrChars(text string) []string {
	out := []string{}
	for _, c := range text {
		out = append(out, string(c))
	}
	return out
}

// StrTrim is String.trim: ASCII whitespace at both ends is removed.
func StrTrim(text string) string {
	return strings.Trim(text, " \t\n\r\f\v")
}

// StrReplace is String.replace.
func StrReplace(text, from, to string) string {
	if from == "" {
		Fail("empty pattern")
	}
	return strings.ReplaceAll(text, from, to)
}

// StrRepeat is String.repeat.
func StrRepeat(text string, times int64) string {
	if times < 0 {
		Fail("negative repeat count")
	}
	return strings.Repeat(text, int(times))
}

// StrUpper is String.to_upper_ascii: only ASCII letters change.
func StrUpper(text string) string {
	return strings.Map(func(c rune) rune {
		if 'a' <= c && c <= 'z' {
			return c - 'a' + 'A'
		}
		return c
	}, text)
}

// StrLower is String.to_lower_ascii: only ASCII letters change.
func StrLower(text string) string {
	return strings.Map(func(c rune) rune {
		if 'A' <= c && c <= 'Z' {
			return c - 'A' + 'a'
		}
		return c
	}, text)
}"#,
    },
    Piece {
        name: "ParseError",
        deps: &[],
        imports: &[],
        code: r#"// ParseError is the error of parse_int and parse_float.
type ParseError struct {
	input string
}

// Message is the text of the error.
func (e ParseError) Message() string {
	return "cannot parse " + e.input
}

func (e ParseError) Error() string {
	return e.Message()
}

// Equal is ==.
func (e ParseError) Equal(other ParseError) bool {
	return e.input == other.input
}

// Copy is .copy().
func (e ParseError) Copy() ParseError {
	return e
}"#,
    },
    Piece {
        name: "parse",
        deps: &["ParseError"],
        imports: &["strconv", "strings"],
        code: r#"func digits(text string, underscores bool) (string, bool) {
	if text == "" || text[0] < '0' || text[0] > '9' {
		return text, false
	}
	i := 0
	for i < len(text) {
		c := text[i]
		if c >= '0' && c <= '9' {
			i++
		} else if underscores && c == '_' && i+1 < len(text) && text[i+1] >= '0' && text[i+1] <= '9' {
			i += 2
		} else {
			break
		}
	}
	return text[i:], true
}

// StringParseInt is String.parse_int: an optional - and decimal digits.
func StringParseInt(text string) (int64, error) {
	rest, ok := digits(strings.TrimPrefix(text, "-"), false)
	if !ok || rest != "" {
		return 0, ParseError{input: text}
	}
	value, err := strconv.ParseInt(text, 10, 64)
	if err != nil {
		return 0, ParseError{input: text}
	}
	return value, nil
}

// StringParseFloat is String.parse_float: the float literal syntax with an optional -, NaN or
// inf.
func StringParseFloat(text string) (float64, error) {
	switch text {
	case "NaN", "inf", "-inf":
		value, _ := strconv.ParseFloat(text, 64)
		return value, nil
	}
	rest, ok := digits(strings.TrimPrefix(text, "-"), true)
	if ok && strings.HasPrefix(rest, ".") {
		rest, ok = digits(rest[1:], true)
	}
	if ok && (strings.HasPrefix(rest, "e") || strings.HasPrefix(rest, "E")) {
		rest = strings.TrimPrefix(strings.TrimPrefix(rest[1:], "+"), "-")
		rest, ok = digits(rest, true)
	}
	if !ok || rest != "" {
		return 0, ParseError{input: text}
	}
	value, _ := strconv.ParseFloat(strings.ReplaceAll(text, "_", ""), 64)
	return value, nil
}"#,
    },
    Piece {
        name: "list",
        deps: &["index", "Panic", "int"],
        imports: &["strconv"],
        code: r#"// Pop is List.pop.
func Pop[T any](items *[]T) (T, bool) {
	n := len(*items)
	if n == 0 {
		var zero T
		return zero, false
	}
	last := (*items)[n-1]
	*items = (*items)[:n-1]
	return last, true
}

// ListInsert is List.insert.
func ListInsert[T any](items *[]T, i int64, item T) {
	if i < 0 || i > int64(len(*items)) {
		Fail("index " + strconv.FormatInt(i, 10) + " out of range for length " + strconv.Itoa(len(*items)))
	}
	*items = append(*items, item)
	copy((*items)[i+1:], (*items)[i:])
	(*items)[i] = item
}

// ListRemove is List.remove.
func ListRemove[T any](items *[]T, i int64) T {
	k := index(i, len(*items))
	item := (*items)[k]
	*items = append((*items)[:k], (*items)[k+1:]...)
	return item
}

// ListSwap is List.swap.
func ListSwap[T any](items []T, i, j int64) {
	a := index(i, len(items))
	b := index(j, len(items))
	items[a], items[b] = items[b], items[a]
}

// ListReverse is List.reverse.
func ListReverse[T any](items []T) {
	for i, j := 0, len(items)-1; i < j; i, j = i+1, j-1 {
		items[i], items[j] = items[j], items[i]
	}
}

// ListMap is List.map.
func ListMap[T, U any](items []T, f func(T) U) []U {
	out := make([]U, 0, len(items))
	for _, item := range items {
		out = append(out, f(item))
	}
	return out
}

// ListFilter is List.filter.
func ListFilter[T any](items []T, keep func(T) bool) []T {
	out := []T{}
	for _, item := range items {
		if keep(item) {
			out = append(out, item)
		}
	}
	return out
}

// ListAny is List.any.
func ListAny[T any](items []T, f func(T) bool) bool {
	for _, item := range items {
		if f(item) {
			return true
		}
	}
	return false
}

// ListAll is List.all.
func ListAll[T any](items []T, f func(T) bool) bool {
	for _, item := range items {
		if !f(item) {
			return false
		}
	}
	return true
}

// ListCount is List.count.
func ListCount[T any](items []T, f func(T) bool) int64 {
	n := int64(0)
	for _, item := range items {
		if f(item) {
			n++
		}
	}
	return n
}

// ListContains is std.list.contains.
func ListContains[T any](items []T, item T, eq func(T, T) bool) bool {
	_, found := ListIndexOf(items, item, eq)
	return found
}

// ListIndexOf is std.list.index_of.
func ListIndexOf[T any](items []T, item T, eq func(T, T) bool) (int64, bool) {
	for i, x := range items {
		if eq(x, item) {
			return int64(i), true
		}
	}
	return 0, false
}

// ListGet is std.list.get.
func ListGet[T any](items []T, i int64, copy func(T) T) (T, bool) {
	if i < 0 || i >= int64(len(items)) {
		var zero T
		return zero, false
	}
	return copy(items[i]), true
}

// ListSum is std.list.sum.
func ListSum(items []int64) int64 {
	total := int64(0)
	for _, item := range items {
		total = Add(total, item)
	}
	return total
}

// ListRange is std.list.range.
func ListRange(from, to int64) []int64 {
	out := []int64{}
	for i := from; i < to; i++ {
		out = append(out, i)
	}
	return out
}"#,
    },
    Piece {
        name: "sort",
        deps: &["Ordering"],
        imports: &["slices"],
        code: r#"// ListSort is std.list.sort: a stable sort.
func ListSort[T any](items []T, compare func(T, T) Ordering) {
	slices.SortStableFunc(items, func(a, b T) int {
		return order(compare(a, b))
	})
}

// ListMin is std.list.min.
func ListMin[T any](items []T, compare func(T, T) Ordering, copy func(T) T) (T, bool) {
	return best(items, compare, copy, OrderingLess)
}

// ListMax is std.list.max.
func ListMax[T any](items []T, compare func(T, T) Ordering, copy func(T) T) (T, bool) {
	return best(items, compare, copy, OrderingGreater)
}

func best[T any](items []T, compare func(T, T) Ordering, copy func(T) T, want Ordering) (T, bool) {
	if len(items) == 0 {
		var zero T
		return zero, false
	}
	b := items[0]
	for _, item := range items[1:] {
		if compare(item, b) == want {
			b = item
		}
	}
	return copy(b), true
}"#,
    },
    Piece {
        name: "Entry",
        deps: &[],
        imports: &[],
        code: r#"// Entry is one key and its value.
type Entry[K, V any] struct {
	Key   K
	Value V
}

// Equal is ==.
func (e Entry[K, V]) Equal(other Entry[K, V], eqK func(K, K) bool, eqV func(V, V) bool) bool {
	return eqK(e.Key, other.Key) && eqV(e.Value, other.Value)
}

// Copy is .copy().
func (e Entry[K, V]) Copy(copyK func(K) K, copyV func(V) V) Entry[K, V] {
	return Entry[K, V]{Key: copyK(e.Key), Value: copyV(e.Value)}
}"#,
    },
    Piece {
        name: "Map",
        deps: &["Entry", "Panic"],
        imports: &[],
        code: r#"// Map is a map that keeps the order in which keys were first inserted (ADR 0003).
type Map[K comparable, V any] struct {
	keys   []K
	values []V
	index  map[K]int
}

// MapOf is a map literal.
func MapOf[K comparable, V any](entries ...Entry[K, V]) Map[K, V] {
	var m Map[K, V]
	for _, e := range entries {
		m.Insert(e.Key, e.Value)
	}
	return m
}

// Len is Map.len.
func (m Map[K, V]) Len() int64 {
	return int64(len(m.keys))
}

// IsEmpty is Map.is_empty.
func (m Map[K, V]) IsEmpty() bool {
	return len(m.keys) == 0
}

// ContainsKey is Map.contains_key.
func (m Map[K, V]) ContainsKey(key K) bool {
	_, ok := m.index[key]
	return ok
}

// Insert is Map.insert: it returns the replaced value.
func (m *Map[K, V]) Insert(key K, value V) (V, bool) {
	if i, ok := m.index[key]; ok {
		old := m.values[i]
		m.values[i] = value
		return old, true
	}
	if m.index == nil {
		m.index = map[K]int{}
	}
	m.index[key] = len(m.keys)
	m.keys = append(m.keys, key)
	m.values = append(m.values, value)
	var zero V
	return zero, false
}

// Remove is Map.remove.
func (m *Map[K, V]) Remove(key K) (V, bool) {
	i, ok := m.index[key]
	if !ok {
		var zero V
		return zero, false
	}
	old := m.values[i]
	m.keys = append(m.keys[:i:i], m.keys[i+1:]...)
	m.values = append(m.values[:i:i], m.values[i+1:]...)
	delete(m.index, key)
	for j := i; j < len(m.keys); j++ {
		m.index[m.keys[j]] = j
	}
	return old, true
}

// Keys is Map.keys.
func (m Map[K, V]) Keys() []K {
	return append([]K{}, m.keys...)
}

// Entries is Map.entries.
func (m Map[K, V]) Entries() []Entry[K, V] {
	out := make([]Entry[K, V], 0, len(m.keys))
	for i, k := range m.keys {
		out = append(out, Entry[K, V]{Key: k, Value: m.values[i]})
	}
	return out
}

// Clear is Map.clear.
func (m *Map[K, V]) Clear() {
	*m = Map[K, V]{}
}

func (m Map[K, V]) at(key K) int {
	i, ok := m.index[key]
	if !ok {
		Fail("key not found in map")
	}
	return i
}

// Get is map[key].
func (m Map[K, V]) Get(key K) V {
	return m.values[m.at(key)]
}

// Set is map[key] = value: it replaces the value of an existing key.
func (m Map[K, V]) Set(key K, value V) {
	m.values[m.at(key)] = value
}

// Ref is map[key] as a place for a mut parameter.
func (m Map[K, V]) Ref(key K) *V {
	return &m.values[m.at(key)]
}

// Copy is .copy().
func (m Map[K, V]) Copy(copy func(V) V) Map[K, V] {
	var out Map[K, V]
	for i, k := range m.keys {
		out.Insert(k, copy(m.values[i]))
	}
	return out
}

// MapEqual is == of two maps: the same keys with equal values, in any order.
func MapEqual[K comparable, V any](a, b Map[K, V], eq func(V, V) bool) bool {
	if len(a.keys) != len(b.keys) {
		return false
	}
	for i, k := range a.keys {
		j, ok := b.index[k]
		if !ok || !eq(a.values[i], b.values[j]) {
			return false
		}
	}
	return true
}

// MapGet is std.map.get.
func MapGet[K comparable, V any](m Map[K, V], key K, copy func(V) V) (V, bool) {
	i, ok := m.index[key]
	if !ok {
		var zero V
		return zero, false
	}
	return copy(m.values[i]), true
}

// MapValues is std.map.values.
func MapValues[K comparable, V any](m Map[K, V], copy func(V) V) []V {
	out := make([]V, 0, len(m.values))
	for _, v := range m.values {
		out = append(out, copy(v))
	}
	return out
}"#,
    },
    Piece {
        name: "FsError",
        deps: &[],
        imports: &[],
        code: r#"// FsError is the error of std.fs.
type FsError interface {
	isFsError()
	Message() string
	Error() string
	Equal(other FsError) bool
	Copy() FsError
}

// FsErrorNotFound is FsError.NotFound: the path.
type FsErrorNotFound struct {
	F0 string
}

// FsErrorPermissionDenied is FsError.PermissionDenied: the path.
type FsErrorPermissionDenied struct {
	F0 string
}

// FsErrorOther is FsError.Other: a message from the target.
type FsErrorOther struct {
	F0 string
}

func (*FsErrorNotFound) isFsError()         {}
func (*FsErrorPermissionDenied) isFsError() {}
func (*FsErrorOther) isFsError()            {}

// Message is the text of the error.
func (e *FsErrorNotFound) Message() string { return "file not found: " + e.F0 }

// Message is the text of the error.
func (e *FsErrorPermissionDenied) Message() string { return "permission denied: " + e.F0 }

// Message is the text of the error.
func (e *FsErrorOther) Message() string { return e.F0 }

func (e *FsErrorNotFound) Error() string         { return e.Message() }
func (e *FsErrorPermissionDenied) Error() string { return e.Message() }
func (e *FsErrorOther) Error() string            { return e.Message() }

// Equal is ==.
func (e *FsErrorNotFound) Equal(other FsError) bool {
	o, ok := other.(*FsErrorNotFound)
	return ok && e.F0 == o.F0
}

// Equal is ==.
func (e *FsErrorPermissionDenied) Equal(other FsError) bool {
	o, ok := other.(*FsErrorPermissionDenied)
	return ok && e.F0 == o.F0
}

// Equal is ==.
func (e *FsErrorOther) Equal(other FsError) bool {
	o, ok := other.(*FsErrorOther)
	return ok && e.F0 == o.F0
}

// Copy is .copy().
func (e *FsErrorNotFound) Copy() FsError { return &FsErrorNotFound{F0: e.F0} }

// Copy is .copy().
func (e *FsErrorPermissionDenied) Copy() FsError { return &FsErrorPermissionDenied{F0: e.F0} }

// Copy is .copy().
func (e *FsErrorOther) Copy() FsError { return &FsErrorOther{F0: e.F0} }"#,
    },
    Piece {
        name: "fs",
        deps: &["FsError"],
        imports: &["errors", "io/fs", "os"],
        code: r#"func fsError(err error, path string) FsError {
	switch {
	case errors.Is(err, fs.ErrNotExist):
		return &FsErrorNotFound{F0: path}
	case errors.Is(err, fs.ErrPermission):
		return &FsErrorPermissionDenied{F0: path}
	}
	return &FsErrorOther{F0: err.Error()}
}

// FsReadText is std.fs.read_text.
func FsReadText(path string) (string, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return "", fsError(err, path)
	}
	return string(data), nil
}

// FsWriteText is std.fs.write_text.
func FsWriteText(path, content string) error {
	if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
		return fsError(err, path)
	}
	return nil
}

// FsExists is std.fs.exists.
func FsExists(path string) bool {
	_, err := os.Stat(path)
	return err == nil
}"#,
    },
];

/// The runtime package with the given pieces and their dependencies, in a fixed order.
pub fn file(used: &BTreeSet<&'static str>, version: &str) -> String {
    let mut needed: BTreeSet<&'static str> = BTreeSet::new();
    let mut stack: Vec<&'static str> = used.iter().copied().collect();
    while let Some(name) = stack.pop() {
        if !needed.insert(name) {
            continue;
        }
        if let Some(piece) = PIECES.iter().find(|p| p.name == name) {
            stack.extend(piece.deps.iter().copied());
        }
    }
    let mut imports: BTreeSet<&str> = BTreeSet::new();
    for piece in PIECES.iter().filter(|p| needed.contains(p.name)) {
        imports.extend(piece.imports.iter().copied());
    }
    let mut out = format!(
        "// Generated by wolf {version}. Do not edit.\n\n// Package wolfrt is the runtime of wolfpack programs in Go, with the declarations of std.\npackage wolfrt\n"
    );
    match imports.len() {
        0 => {}
        1 => {
            for i in &imports {
                out.push_str(&format!("\nimport \"{i}\"\n"));
            }
        }
        _ => {
            out.push_str("\nimport (\n");
            for i in &imports {
                out.push_str(&format!("\t\"{i}\"\n"));
            }
            out.push_str(")\n");
        }
    }
    for piece in PIECES.iter().filter(|p| needed.contains(p.name)) {
        out.push('\n');
        out.push_str(piece.code);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_dependency_exists() {
        for p in super::PIECES {
            for d in p.deps {
                assert!(
                    super::PIECES.iter().any(|q| q.name == *d),
                    "{} needs {d}",
                    p.name
                );
            }
        }
    }
}
