use pgrx::prelude::*;

#[pg_schema]
mod neon_ai {
    use super::super::errors::*;
    use super::super::json_api::*;
    use pgrx::prelude::*;
    use serde::{Deserialize, Serialize};

    // API key

    extension_sql!(
        "CREATE FUNCTION neon_ai.openai_set_api_key(api_key text) RETURNS void
        LANGUAGE SQL VOLATILE STRICT AS $$
            INSERT INTO neon_ai.config VALUES ('OPENAI_KEY', api_key)
            ON CONFLICT (name) DO UPDATE SET value = EXCLUDED.value;
        $$;
        CREATE FUNCTION neon_ai.openai_get_api_key() RETURNS text
        LANGUAGE SQL VOLATILE STRICT AS $$
            SELECT value FROM neon_ai.config WHERE name = 'OPENAI_KEY';
        $$;",
        name = "openai_api_key",
        requires = ["config"],
    );

    // embeddings

    #[derive(Serialize)]
    struct OpenAIEmbeddingReq {
        model: String,
        input: String,
    }
    #[derive(Deserialize)]
    struct OpenAIEmbeddingData {
        data: Vec<OpenAIEmbedding>,
    }
    #[derive(Deserialize)]
    struct OpenAIEmbedding {
        embedding: Vec<f32>,
    }

    #[pg_extern(immutable, strict)]
    pub fn _openai_text_embedding(model: &str, input: &str, key: &str) -> Vec<f32> {
        let body = OpenAIEmbeddingReq {
            model: model.to_string(),
            input: input.to_string(),
        };

        let json = json_api("https://api.openai.com/v1/embeddings", key, body);
        let embed_data: OpenAIEmbeddingData =
            serde_json::from_value(json).expect_or_pg_err("Unexpected JSON structure in OpenAI response");

        embed_data
            .data
            .into_iter()
            .next()
            .unwrap_or_pg_err("No embedding object in OpenAI response")
            .embedding
    }

    extension_sql!(
        "CREATE FUNCTION neon_ai.openai_text_embedding(model text, input text) RETURNS vector
        LANGUAGE PLPGSQL IMMUTABLE STRICT AS $$
            DECLARE
                api_key text := neon_ai.openai_get_api_key();
                res vector;
            BEGIN
                IF api_key IS NULL THEN
                    RAISE EXCEPTION '[neon_ai] OpenAI API key is not set';
                END IF;
                SELECT neon_ai._openai_text_embedding(model, input, api_key)::vector INTO res;
                RETURN res;
            END;
        $$;
        CREATE FUNCTION neon_ai.openai_text_embedding_ada_002(input text) RETURNS vector(1536)
        LANGUAGE SQL IMMUTABLE STRICT AS $$
          SELECT neon_ai.openai_text_embedding('text-embedding-ada-002', input)::vector(1536);
        $$;
        CREATE FUNCTION neon_ai.openai_text_embedding_3_small(input text) RETURNS vector(1536)
        LANGUAGE SQL IMMUTABLE STRICT AS $$
          SELECT neon_ai.openai_text_embedding('text-embedding-3-small', input)::vector(1536);
        $$;
        CREATE FUNCTION neon_ai.openai_text_embedding_3_large(input text) RETURNS vector(3072)
        LANGUAGE SQL IMMUTABLE STRICT AS $$
          SELECT neon_ai.openai_text_embedding('text-embedding-3-large', input)::vector(3072);
        $$;",
        name = "openai_embeddings",
    );

    // ChatGPT

    #[pg_extern(strict)]
    pub fn _openai_chat_completion(json_body: pgrx::Json, key: &str) -> pgrx::Json {
        let json = json_api("https://api.openai.com/v1/chat/completions", key, json_body);
        pgrx::Json(json)
    }

    extension_sql!(
        "CREATE FUNCTION neon_ai.openai_chat_completion(body json) RETURNS json
        LANGUAGE PLPGSQL VOLATILE STRICT AS $$
            DECLARE
                api_key text := neon_ai.openai_get_api_key();
                res json;
            BEGIN
                IF api_key IS NULL THEN
                    RAISE EXCEPTION '[neon_ai] OpenAI API key is not set';
                END IF;
                SELECT neon_ai._openai_chat_completion(body, api_key) INTO res;
                RETURN res;
            END;
        $$;",
        name = "openai_chat_completion",
    );
}

#[cfg(any(test, feature = "pg_test"))]
#[pg_schema]
mod tests {
    use super::neon_ai::*;
    use pgrx::prelude::*;
    use std::env;

    fn openai_api_key() -> String {
        match env::var("OPENAI_API_KEY") {
            Err(err) => error!("Tests require environment variable OPENAI_API_KEY: {}", err),
            Ok(key) => key,
        }
    }

    #[pg_test(error = "[neon_ai] HTTP status code 401 trying to reach API")]
    fn test_embedding_openai_raw_bad_key() {
        _openai_text_embedding("text-embedding-3-small", "hello world!", "invalid-key");
    }

    #[pg_test(error = "[neon_ai] HTTP status code 404 trying to reach API")]
    fn test_embedding_openai_raw_bad_model() {
        _openai_text_embedding("text-embedding-3-immense", "hello world!", &openai_api_key());
    }

    #[pg_test]
    fn test_embedding_openai_raw_has_data() {
        let embedding = _openai_text_embedding("text-embedding-3-small", "hello world!", &openai_api_key());
        assert_eq!(embedding.len(), 1536);
    }
}
