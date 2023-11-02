// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.
// SPDX-License-Identifier: MIT

use digital_twin_model::trailer_v1;

use env_logger::{Builder, Target};
use interfaces::chariott::service_discovery::core::v1::service_registry_client::ServiceRegistryClient;
use interfaces::chariott::service_discovery::core::v1::DiscoverRequest;
use interfaces::invehicle_digital_twin::v1::invehicle_digital_twin_client::InvehicleDigitalTwinClient;
use interfaces::invehicle_digital_twin::v1::{EndpointInfo, EntityAccessInfo, RegisterRequest};
use log::{debug, info, LevelFilter};
use smart_trailer_interfaces::trailer_connected_provider::v1::trailer_connected_provider_server::TrailerConnectedProviderServer;
use std::net::SocketAddr;
use tokio::signal;
use tonic::transport::Server;
use tonic::{Request, Status};
use trailer_connected_provider_impl::TrailerConnectedProviderImpl;

mod trailer_connected_provider_impl;

const GRPC_PROTOCOL: &str = "grpc";
const OPERATION_GET: &str = "Get";

// TODO: These could be added in configuration
const SERVICE_DISCOVERY_URI: &str = "http://0.0.0.0:50000";
const PROVIDER_AUTHORITY: &str = "0.0.0.0:55000";

pub const INVEHICLE_DIGITAL_TWIN_SERVICE_NAMESPACE: &str = "sdv.ibeji";
pub const INVEHICLE_DIGITAL_TWIN_SERVICE_NAME: &str = "invehicle_digital_twin";
pub const INVEHICLE_DIGITAL_TWIN_SERVICE_VERSION: &str = "1.0";
pub const INVEHICLE_DIGITAL_TWIN_SERVICE_COMMUNICATION_KIND: &str = "grpc+proto";
pub const INVEHICLE_DIGITAL_TWIN_SERVICE_COMMUNICATION_REFERENCE: &str = "https://github.com/eclipse-ibeji/ibeji/blob/main/interfaces/digital_twin/v1/digital_twin.proto";

/// Use Chariott Service Discovery to discover a service.
///
/// # Arguments
/// * `chariott_uri` - Chariott's URI.
/// * `namespace` - The service's namespace.
/// * `name` - The service's name.
/// * `version` - The service's version.
/// # `communication_kind` - The service's communication kind.
/// # `communication_reference` - The service's communication reference.
pub async fn discover_service_using_chariott(
    chariott_uri: &str,
    namespace: &str,
    name: &str,
    version: &str,
    communication_kind: &str,
    communication_reference: &str,
) -> Result<String, Status> {
    let mut client = ServiceRegistryClient::connect(chariott_uri.to_string())
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    let request = Request::new(DiscoverRequest {
        namespace: namespace.to_string(),
        name: name.to_string(),
        version: version.to_string(),
    });

    let response = client
        .discover(request)
        .await
        .map_err(|error| Status::internal(error.to_string()))?;

    let service = response.into_inner().service.ok_or_else(|| Status::not_found("Did not find a service in Chariott with namespace '{namespace}', name '{name}' and version {version}"))?;

    if service.communication_kind != communication_kind
        && service.communication_reference != communication_reference
    {
        return Err(Status::not_found(
            "Did not find a service in Chariott with namespace '{namespace}', name '{name}' and version {version} that has communication kind '{communication_kind} and communication_reference '{communication_reference}''",
        ));
    }

    Ok(service.uri)
}

/// Register the "is trailer connected" property's endpoint.
///
/// # Arguments
/// * `invehicle_digital_twin_uri` - The In-Vehicle Digital Twin URI.
/// * `provider_uri` - The provider's URI.
async fn register_entity(
    invehicle_digital_twin_uri: &str,
    provider_uri: &str,
) -> Result<(), Status> {
    let is_trailer_connected_endpoint_info = EndpointInfo {
        protocol: GRPC_PROTOCOL.to_string(),
        operations: vec![OPERATION_GET.to_string()],
        uri: provider_uri.to_string(),
        context: trailer_v1::trailer::is_trailer_connected::ID.to_string(),
    };
    let entity_access_info = EntityAccessInfo {
        name: trailer_v1::trailer::is_trailer_connected::NAME.to_string(),
        id: trailer_v1::trailer::is_trailer_connected::ID.to_string(),
        description: trailer_v1::trailer::is_trailer_connected::DESCRIPTION.to_string(),
        endpoint_info_list: vec![is_trailer_connected_endpoint_info],
    };

    let mut client = InvehicleDigitalTwinClient::connect(invehicle_digital_twin_uri.to_string())
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
    let request = tonic::Request::new(RegisterRequest {
        entity_access_info_list: vec![entity_access_info],
    });
    let _response = client.register(request).await?;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Set up logging.
    Builder::new()
        .filter(None, LevelFilter::Debug)
        .target(Target::Stdout)
        .init();

    info!("The Provider has started.");

    let provider_uri = format!("http://{PROVIDER_AUTHORITY}");
    debug!("The Provider URI is {}", &provider_uri);

    // Setup the HTTP server.
    let addr: SocketAddr = PROVIDER_AUTHORITY.parse()?;
    let provider_impl = TrailerConnectedProviderImpl::default();
    let server_future = Server::builder()
        .add_service(TrailerConnectedProviderServer::new(provider_impl))
        .serve(addr);
    info!("The HTTP server is listening on address '{PROVIDER_AUTHORITY}'");

    // Get the In-vehicle Digital Twin Uri from the service discovery system
    // This could be enhances to add retries for robustness
    let invehicle_digital_twin_uri = discover_service_using_chariott(
        SERVICE_DISCOVERY_URI,
        INVEHICLE_DIGITAL_TWIN_SERVICE_NAMESPACE,
        INVEHICLE_DIGITAL_TWIN_SERVICE_NAME,
        INVEHICLE_DIGITAL_TWIN_SERVICE_VERSION,
        INVEHICLE_DIGITAL_TWIN_SERVICE_COMMUNICATION_KIND,
        INVEHICLE_DIGITAL_TWIN_SERVICE_COMMUNICATION_REFERENCE,
    )
    .await?;

    debug!("Sending a register request to the In-Vehicle Digital Twin Service URI {invehicle_digital_twin_uri}");

    // This could be enhanced to add retries for robustness
    register_entity(&invehicle_digital_twin_uri, &provider_uri).await?;
    server_future.await?;

    signal::ctrl_c()
        .await
        .expect("Failed to listen for control-c event");

    info!("The Provider has completed.");

    Ok(())
}
