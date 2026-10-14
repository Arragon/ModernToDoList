$headers = @{
    'Authorization' = '$env:LINEAR_API_KEY'
    'Content-Type' = 'application/json'
}

# Query workflow states for the Inhandy team
$query = @'
{
  "query": "query { team(id: \"8687f6d2-946e-4927-9f7b-59ee965e2777\") { name states { nodes { id name type color position } } } }"
}
'@

$response = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $query
$response | ConvertTo-Json -Depth 10
