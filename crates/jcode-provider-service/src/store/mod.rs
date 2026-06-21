//! Concrete [`CredentialService`] implementations.

pub mod in_memory;
pub mod integration;
pub mod keyring;
pub mod service;

pub use in_memory::InMemoryCredentialStore;
pub use integration::PersistentIntegration;
pub use keyring::KeyringCredentialStore;
pub use service::DefaultProviderService;
