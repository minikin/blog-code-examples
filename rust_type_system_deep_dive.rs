// RUST TYPE SYSTEM DEEP DIVE EXAMPLES
// This file compiles all code examples from the blog post to validate compilation

use std::marker::PhantomData;

// ----------------- GENERIC ASSOCIATED TYPES (GATs) EXAMPLES -----------------

// Basic GAT example
trait Container {
    type Item<'a>
    where
        Self: 'a;
    fn get<'a>(&'a self) -> Option<Self::Item<'a>>;
}

impl<T> Container for Vec<T> {
    type Item<'a>
        = &'a T
    where
        Self: 'a;

    fn get<'a>(&'a self) -> Option<Self::Item<'a>> {
        self.first()
    }
}

// CollectionFactory with GATs
trait CollectionFactory {
    type Collection<'a>
    where
        Self: 'a;
    type Iterator<'a>: Iterator
    where
        Self: 'a;

    fn create_collection<'a>(&'a self) -> Self::Collection<'a>;
    fn iter<'a>(&'a self) -> Self::Iterator<'a>;
}

struct VecFactory<T>(Vec<T>);

impl<T: Clone> CollectionFactory for VecFactory<T> {
    type Collection<'a>
        = Vec<T>
    where
        T: 'a;
    type Iterator<'a>
        = std::slice::Iter<'a, T>
    where
        T: 'a;

    fn create_collection<'a>(&'a self) -> Vec<T> {
        self.0.clone()
    }

    fn iter<'a>(&'a self) -> std::slice::Iter<'a, T> {
        self.0.iter()
    }
}

// ----------------- ADVANCED LIFETIME MANAGEMENT EXAMPLES -----------------

// Higher-Rank Trait Bounds example
trait Parser {
    fn parse<F, O>(&self, f: F) -> O
    where
        F: for<'a> FnOnce(&'a str) -> O;
}

struct SimpleParser;

impl Parser for SimpleParser {
    fn parse<F, O>(&self, f: F) -> O
    where
        F: for<'a> FnOnce(&'a str) -> O,
    {
        let data = "sample data";
        f(data)
    }
}

// Lifetime variance with tokens
struct AdminToken<'a>(&'a str);
struct UserToken<'a>(&'a str);

fn check_admin_access(_token: AdminToken) -> bool {
    // Verification logic
    true
}

fn provide_longer_lived<'long>(long_lived: &'long u32) {
    needs_short_lived(long_lived); // This works because of covariance
}

fn needs_short_lived<'a, 'b: 'a>(_data: &'a u32) {
    // Some code
}

// ----------------- PHANTOM TYPES EXAMPLES -----------------

// Basic phantom type for tokens
struct Token<State> {
    value: String,
    _state: PhantomData<State>,
}

// States (empty structs)
struct Unvalidated;
struct Validated;

// Validation error type
enum ValidationError {
    TooShort,
    InvalidFormat,
}

impl Token<Unvalidated> {
    fn new(value: String) -> Self {
        Token {
            value,
            _state: PhantomData,
        }
    }

    fn validate(self) -> Result<Token<Validated>, ValidationError> {
        // Perform validation
        if self.value.len() > 3 {
            Ok(Token {
                value: self.value,
                _state: PhantomData,
            })
        } else {
            Err(ValidationError::TooShort)
        }
    }
}

impl Token<Validated> {
    fn use_validated(&self) -> &str {
        // Only callable on validated tokens
        &self.value
    }
}

// Type-level validation with phantom types
struct UserId<Validated>(String, PhantomData<Validated>);
struct EmailAddress<Validated>(String, PhantomData<Validated>);

struct Unverified;
struct Verified;

trait Validator<T> {
    type Error;
    fn validate(value: String) -> Result<T, Self::Error>;
}

struct UserIdValidator;
impl Validator<UserId<Verified>> for UserIdValidator {
    type Error = String;

    fn validate(value: String) -> Result<UserId<Verified>, Self::Error> {
        if value.len() >= 3 && value.chars().all(|c| c.is_alphanumeric()) {
            Ok(UserId(value, PhantomData))
        } else {
            Err("Invalid user ID".to_string())
        }
    }
}

// ----------------- TYPECLASS PATTERN EXAMPLES -----------------

// Monoid and Semigroup typeclasses
trait Semigroup {
    fn combine(&self, other: &Self) -> Self;
}

trait Monoid: Semigroup + Clone {
    fn empty() -> Self;
}

// Implementing for built-in types
impl Semigroup for String {
    fn combine(&self, other: &Self) -> Self {
        let mut result = self.clone();
        result.push_str(other);
        result
    }
}

impl Monoid for String {
    fn empty() -> Self {
        String::new()
    }
}

// Product and Sum types for numbers
#[derive(Clone)]
struct Product<T>(T);

#[derive(Clone)]
struct Sum<T>(T);

impl<T: Clone + std::ops::Mul<Output = T>> Semigroup for Product<T> {
    fn combine(&self, other: &Self) -> Self {
        Product(self.0.clone() * other.0.clone())
    }
}

impl<T: Clone + std::ops::Mul<Output = T> + From<u8>> Monoid for Product<T> {
    fn empty() -> Self {
        Product(T::from(1))
    }
}

impl<T: Clone + std::ops::Add<Output = T>> Semigroup for Sum<T> {
    fn combine(&self, other: &Self) -> Self {
        Sum(self.0.clone() + other.0.clone())
    }
}

impl<T: Clone + std::ops::Add<Output = T> + From<u8>> Monoid for Sum<T> {
    fn empty() -> Self {
        Sum(T::from(0))
    }
}

// Generic algorithm with typeclasses
fn combine_all<M: Monoid + Clone>(values: &[M]) -> M {
    values.iter().fold(M::empty(), |acc, x| acc.combine(x))
}

// Functor typeclass emulation
trait Functor {
    type Item<T>;

    fn map<A, B, F>(container: &Self::Item<A>, f: F) -> Self::Item<B>
    where
        F: FnMut(A) -> B,
        A: Clone;
}

struct OptionFunctor;
impl Functor for OptionFunctor {
    type Item<T> = Option<T>;

    fn map<A, B, F>(container: &Self::Item<A>, mut f: F) -> Self::Item<B>
    where
        F: FnMut(A) -> B,
        A: Clone,
    {
        container.as_ref().map(|x| f(x.clone()))
    }
}

struct VecFunctor;
impl Functor for VecFunctor {
    type Item<T> = Vec<T>;

    fn map<A, B, F>(container: &Self::Item<A>, mut f: F) -> Self::Item<B>
    where
        F: FnMut(A) -> B,
        A: Clone,
    {
        container.iter().map(|x| f(x.clone())).collect()
    }
}

// Generic code with functors
fn transform_and_process<F, A, B>(
    _functor: F,
    container: &F::Item<A>,
    transform: impl FnMut(A) -> B,
) -> F::Item<B>
where
    F: Functor,
    A: Clone,
    B: Clone,
{
    F::map(container, transform)
}

// ----------------- ZERO-SIZED TYPES EXAMPLES -----------------

// ZST for database access levels
struct ReadOnly;
struct ReadWrite;

struct Database<Access> {
    connection_string: String,
    _marker: PhantomData<Access>,
}

impl<Access> Database<Access> {
    fn query(&self, query: &str) -> Vec<String> {
        // Common query logic
        vec![format!("Result of {}", query)]
    }
}

impl Database<ReadWrite> {
    fn execute(&self, _command: &str) -> Result<(), String> {
        // Only available in read-write mode
        Ok(())
    }
}

// Type-level state machine with ZSTs
struct Draft;
struct Published;
struct Archived;

struct Post<State> {
    content: String,
    _state: PhantomData<State>,
}

impl Post<Draft> {
    fn new(content: String) -> Self {
        Post {
            content,
            _state: PhantomData,
        }
    }

    fn edit(&mut self, content: String) {
        self.content = content;
    }

    fn publish(self) -> Post<Published> {
        Post {
            content: self.content,
            _state: PhantomData,
        }
    }
}

impl Post<Published> {
    fn get_views(&self) -> u64 {
        42 // Placeholder
    }

    fn archive(self) -> Post<Archived> {
        Post {
            content: self.content,
            _state: PhantomData,
        }
    }
}

impl Post<Archived> {
    fn restore(self) -> Post<Draft> {
        Post {
            content: self.content,
            _state: PhantomData,
        }
    }
}

// Type-level integers with const generics
struct Length<const METERS: i32, const CENTIMETERS: i32>;

// Type-level representation of physical quantities
impl<const M: i32, const CM: i32> Length<M, CM> {
    // A const function to calculate total centimeters (for demonstration)
    const fn total_cm() -> i32 {
        M * 100 + CM
    }
}

// Type-safe addition using type conversion rather than type-level arithmetic
fn add<const M1: i32, const CM1: i32, const M2: i32, const CM2: i32>(
    _: Length<M1, CM1>,
    _: Length<M2, CM2>,
) -> Length<3, 120> {
    // Using fixed return type for the example
    // In a real implementation, we would define constant expressions and
    // use const generics with a more flexible type, but that gets complex
    Length
}

// ----------------- TYPE ERASURE EXAMPLES -----------------

// Dynamic dispatch with trait objects
trait Drawable {
    fn draw(&self);
    fn bounding_box(&self) -> BoundingBox;
}

struct BoundingBox {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

struct Rectangle {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl Drawable for Rectangle {
    fn draw(&self) {
        // Draw the rectangle
    }

    fn bounding_box(&self) -> BoundingBox {
        BoundingBox {
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
        }
    }
}

struct Circle {
    x: f32,
    y: f32,
    radius: f32,
}

impl Drawable for Circle {
    fn draw(&self) {
        // Draw the circle
    }

    fn bounding_box(&self) -> BoundingBox {
        BoundingBox {
            x: self.x - self.radius,
            y: self.y - self.radius,
            width: self.radius * 2.0,
            height: self.radius * 2.0,
        }
    }
}

struct Canvas {
    // A collection of drawable objects with different concrete types
    elements: Vec<Box<dyn Drawable>>,
}

impl Canvas {
    fn new() -> Self {
        Canvas {
            elements: Vec::new(),
        }
    }

    fn add_element<T: Drawable + 'static>(&mut self, element: T) {
        self.elements.push(Box::new(element));
    }

    fn draw_all(&self) {
        for element in &self.elements {
            element.draw();
        }
    }
}

// Static type erasure
struct ErasedFn<Args, Output> {
    // Pointer to the function implementation
    call_fn: fn(*const (), Args) -> Output,
    // Pointer to the data, with concrete type erased
    data: *const (),
    // Destructor function
    drop_fn: fn(*const ()),
    // Clone function
    clone_fn: fn(*const ()) -> *const (),
}

impl<Args, Output> ErasedFn<Args, Output> {
    // Create a new type-erased function from any suitable closure
    pub fn new<F>(f: F) -> Self
    where
        F: Fn(Args) -> Output + Clone,
    {
        // Implementation function that knows how to call F
        fn call_impl<F, Args, Output>(data: *const (), args: Args) -> Output
        where
            F: Fn(Args) -> Output,
        {
            let f = unsafe { &*(data as *const F) };
            f(args)
        }

        // Drop implementation that knows how to drop F
        fn drop_impl<F>(data: *const ()) {
            unsafe {
                std::ptr::drop_in_place(data as *mut F);
            }
        }

        // Clone implementation that knows how to clone F
        fn clone_impl<F>(data: *const ()) -> *const ()
        where
            F: Clone,
        {
            let f = unsafe { &*(data as *const F) };
            let cloned = f.clone();
            let boxed = Box::new(cloned);
            Box::into_raw(boxed) as *const ()
        }

        let boxed = Box::new(f);
        let data = Box::into_raw(boxed) as *const ();

        ErasedFn {
            call_fn: call_impl::<F, Args, Output>,
            data,
            drop_fn: drop_impl::<F>,
            clone_fn: clone_impl::<F>,
        }
    }

    // Call the wrapped function
    pub fn call(&self, args: Args) -> Output {
        (self.call_fn)(self.data, args)
    }
}

// Implement proper cleanup
impl<Args, Output> Drop for ErasedFn<Args, Output> {
    fn drop(&mut self) {
        (self.drop_fn)(self.data);
    }
}

// Allow cloning of type-erased functions
impl<Args, Output> Clone for ErasedFn<Args, Output> {
    fn clone(&self) -> Self {
        let new_data = (self.clone_fn)(self.data);
        ErasedFn {
            call_fn: self.call_fn,
            data: new_data,
            drop_fn: self.drop_fn,
            clone_fn: self.clone_fn,
        }
    }
}

// Object-safe trait pattern
trait NonObjectSafe {
    fn process<T: std::fmt::Debug>(&self, value: T);
}

// Object-safe wrapper
trait ObjectSafe {
    fn process_i32(&self, value: i32);
    fn process_string(&self, value: String);
    // Add concrete methods for each type you need
}

// Bridge implementation
impl<T: NonObjectSafe> ObjectSafe for T {
    fn process_i32(&self, value: i32) {
        self.process(value);
    }

    fn process_string(&self, value: String) {
        self.process(value);
    }
}

// Example implementation
struct Processor;

impl NonObjectSafe for Processor {
    fn process<T: std::fmt::Debug>(&self, value: T) {
        println!("Processing: {:?}", value);
    }
}

// Heterogeneous collections
trait Message {
    fn process(&self);
}

// Type-erased message holder
struct AnyMessage {
    inner: Box<dyn Message>,
}

// Specific message types
struct TextMessage(String);
struct BinaryMessage(Vec<u8>);

impl Message for TextMessage {
    fn process(&self) {
        println!("Processing text: {}", self.0);
    }
}

impl Message for BinaryMessage {
    fn process(&self) {
        println!("Processing binary data of size: {}", self.0.len());
    }
}

// Enum-based type erasure
enum MessageKind {
    Text(String),
    Binary(Vec<u8>),
}

impl MessageKind {
    fn process(&self) {
        match self {
            MessageKind::Text(text) => {
                println!("Processing text: {}", text);
            }
            MessageKind::Binary(data) => {
                println!("Processing binary data of size: {}", data.len());
            }
        }
    }
}

// Main function to verify some of the examples
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Test GATs
    let vec_data = vec![1, 2, 3];
    let vec_container = vec_data;
    let first_item = vec_container.get().ok_or("No items in container")?;
    println!("First item: {}", first_item);

    // Test phantom types for validation
    let token = Token::<Unvalidated>::new("token12345".to_string());
    match token.validate() {
        Ok(validated) => {
            println!("Token is valid: {}", validated.use_validated());
        }
        Err(_) => {
            println!("Token validation failed");
        }
    }

    // Test typeclasses
    let strings = vec![
        "Hello, ".to_string(),
        "typeclasses ".to_string(),
        "in Rust!".to_string(),
    ];
    let result = combine_all(&strings);
    println!("Combined string: {}", result);

    let numbers = vec![Sum(1), Sum(2), Sum(3), Sum(4)];
    let sum_result = combine_all(&numbers);
    println!("Sum result: {}", sum_result.0);

    // Test ZSTs and state machine
    let mut post = Post::<Draft>::new("Draft content".to_string());
    post.edit("Edited draft content".to_string());
    let published_post = post.publish();
    println!("Published post has {} views", published_post.get_views());

    // Test type-level integers
    let a = Length::<1, 50> {};
    let b = Length::<2, 70> {};
    let _c = add(a, b);

    // Test type erasure
    let mut canvas = Canvas::new();
    canvas.add_element(Rectangle {
        x: 0.0,
        y: 0.0,
        width: 10.0,
        height: 20.0,
    });
    canvas.add_element(Circle {
        x: 15.0,
        y: 15.0,
        radius: 5.0,
    });
    canvas.draw_all();

    // Test static type erasure
    let erased_fn = ErasedFn::new(|x: i32| x * 2);
    println!("ErasedFn result: {}", erased_fn.call(21));

    Ok(())
}
