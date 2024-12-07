Autonomic is a modular operations management framework for building automated management systems.

Autonomic’s theoretical model is based on the principles of autonomic computing for clear skies, full of radiant stars, but not for murky clouds hastily pieced together in quick-and-dirty languages.

Created out of curiosity and for personal use as part of ongoing transition to Rust for implementing infrastructure software.

## 🚧Important🚧
* No stable data model. It is subject to major changes without prior notice.
* Many components are not public yet or partially public.

## Core Design Principles:
* Modular and composable with optional modules.
* Concurrent from the ground up.
* Easy to use and reason about.
* Stingy with resources and efficient in execution.

## Implementation Highlights:
Autonomic consists of multiple modules that can be used to build an automated management system.

### Core Module
The core module has the following main components:

* Operation: Operations are **concurrent** and **independent** **execution contexts** that encapsulate metadata and execution logic.<br>
Operations are defined by implementing the `Operation` trait and managed by `OperationContainer`.<br>
The trait-based approach as opposed to using normal functions, allows more flexibility and extensibility with ability to make operations stateful.<br>
Each operation can have an optional `Sensor` that defines condition, when met, the operation shall be activated.<br>
Operations also support **dynamic runtime parameters** and can be activated with **different values** each time.<br>
It is up to the operation to decide how to handle the parameters, which can be **ignored** or **rejected** with an error message.<br>
Panics in operations **don't cause termination**, and they are caught by the container and dispatched as failed result.
However, when operation panics, it will be **locked** and no further activation will be allowed until manual unlocking.<br><br>

* Operation Controller: An API that manages access to operations on its node. It acts as a registry and namespace for a set of operations.<br>
When an operation is submitted to the controller, it is assigned a container instance that lives as long as the controller lives.<br><br>

* Operation container: `OperationContainer` represents the operation and controls its associated components.<br>
Containers are private and managed internally by the system.<br><br>

* Effector: `Effector` is the controlling wrapper around operations inside `OperationContainer`, and it is responsible for the activation and state management of operations.<br>
Effectors are private and managed internally by the system.<br><br>

* Sensor: `Sensor` is an observer that encapsulates an activation condition.<br>
Activation conditions are types that determine when an operation should be activated, and they are defined by implementing the `ActivationCondition` trait, which allow unconstrained definition of conditions.<br>
Pre-defined conditions are optional feature and users are expected to implement their own conditions using the `ActivationCondition` trait.<br><br>

* Service: traits that define services for specific components.<br><br>

* Data: Lightweight persistence framework that defines standard APIs to persist and query data using Rust's type system, regardless of the storage backend.<br>
It provides `DefaultStore` type that stores data in local database, but users can implement their own data store that works with their storage backend (DBMS or something else) by implementing `DataStore` trait.<br>
Data framework is currently work-in-progress and might be published as **separate crate**.<br><br>

* Tracing: Types and macros to support structured tracing of events.<br><br>

* Errors: Errors of the core components.<br><br>

* Shared traits.<br><br>

### Optional Modules:

The **current** optional models are:

* Service: Concrete implementations of services and other components to support building remote system services.<br>
Meanwhile, I have implemented an **OpenAPI service** with **router**, **server with TLS support** and **client** components, which was originally intended for testing since it is the easiest to change.<br>
The gRPC service implementation is considered for the future, but it is not a priority.<br><br>

* Events: Tracing and monitoring APIs that allow systematic storage and retrieval of events with a set of layers and services.<br><br>

* Operations (only **demonstrative** at this stage): Pre-defined operations for common use-cases.<br><br>

* Conditions (only **demonstrative** at this stage): Pre-defined set of conditions for common use-cases.<br><br>

More is coming as the project evolves.

Development is ongoing in the following areas:
* Data framework.
* Tracing and monitoring framework.
* Planing and management framework.
* CLI
