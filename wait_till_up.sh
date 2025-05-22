until curl -s -o /dev/null -w "%{http_code}" http://localhost:7000/ping | grep -q "200"; do
  echo "Waiting for endpoint..."
  sleep 2
done

echo "Endpoint is up!"
