//! Compile tests for soundness issues that were fixed.  These are only
//! compiled as doctests.

/// A driver must not outlive the sink it drives.
///
/// ```compile_fail,E0597
/// use deser::de::DeserializeDriver;
/// use deser::Deserialize;
///
/// let mut driver = {
///     let mut out = None::<Vec<u32>>;
///     DeserializeDriver::from_sink(Deserialize::deserialize_into(&mut out))
/// };
/// driver.emit(1u64).unwrap();
/// ```
///
/// The sink held by an owned sink cannot be replaced.
///
/// ```compile_fail,E0277
/// use deser::de::{OwnedSink, SinkHandle};
/// use deser::Deserialize;
///
/// let mut owned = OwnedSink::<u32>::deserialize();
/// let mut local = None::<u32>;
/// *owned.borrow_mut() = Deserialize::deserialize_into(&mut local);
/// ```
///
/// Descriptors of sinks cannot borrow from the sink as the deserializer
/// state holds on to them while the sink is mutated.
///
/// ```compile_fail
/// use deser::de::Sink;
/// use deser::Descriptor;
///
/// struct MyDescriptor(String);
///
/// impl Descriptor for MyDescriptor {}
///
/// struct MySink {
///     descriptor: MyDescriptor,
/// }
///
/// impl Sink for MySink {
///     fn descriptor(&self) -> &'static dyn Descriptor {
///         &self.descriptor
///     }
/// }
/// ```
pub struct SoundnessTests;
