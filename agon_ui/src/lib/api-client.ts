import createFetchClient from "openapi-fetch";
import createClient from "openapi-react-query";
import type { paths } from "@/types/api";
import { supabase } from "./supabase";

const fetchClient = createFetchClient<paths>({
  baseUrl: '/api',
});

// Configure authentication middleware
fetchClient.use({
  async onRequest({ request }) {
    const { data: { session } } = await supabase.auth.getSession();
    const token = session?.access_token;
    
    if (token) {
      request.headers.set('Authorization', `Bearer ${token}`);
    }
    
    return request;
  },
});

export const $api = createClient(fetchClient);
export { fetchClient };
