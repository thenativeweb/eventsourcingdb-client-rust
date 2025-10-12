use crate::error::ClientError;

pub fn validate_server_header(response: &reqwest::Response) -> Result<(), ClientError> {
    let server_header = response.headers().get("Server");

    eprintln!("DEBUG: validate_server_header called");
    eprintln!("DEBUG: Server header: {:?}", server_header);

    match server_header {
        None => {
            eprintln!("DEBUG: Server header is None - returning InvalidServerHeader");
            Err(ClientError::InvalidServerHeader)
        }
        Some(header_value) => {
            let header_str = header_value.to_str().unwrap_or("");
            eprintln!("DEBUG: Server header value: '{}'", header_str);
            if header_str.starts_with("EventSourcingDB/") {
                eprintln!("DEBUG: Server header validation successful");
                Ok(())
            } else {
                eprintln!("DEBUG: Server header does not start with 'EventSourcingDB/' - returning InvalidServerHeader");
                Err(ClientError::InvalidServerHeader)
            }
        }
    }
}
