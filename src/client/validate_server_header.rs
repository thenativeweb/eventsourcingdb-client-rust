use crate::error::ClientError;

pub fn validate_server_header(response: &reqwest::Response) -> Result<(), ClientError> {
    let server_header = response.headers().get("Server");

    match server_header {
        None => Err(ClientError::InvalidServerHeader),
        Some(header_value) => {
            let header_str = header_value.to_str().unwrap_or("");
            if header_str.starts_with("EventSourcingDB/") {
                Ok(())
            } else {
                Err(ClientError::InvalidServerHeader)
            }
        }
    }
}
