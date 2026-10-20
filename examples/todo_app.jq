# A to-do REST API built with jqpress. State (the to-do list) is threaded from
# request to request by serve_state — no mutation anywhere.
#
#   jq-llvm -f examples/todo_app.jq -o todo
#   ./todo -n --arg port 8080
#   curl -s localhost:8080/api/todos -d '{"title":"write a web framework in jq"}' -H 'Content-Type: application/json'
#   curl -s localhost:8080/api/todos
include "jqpress";

def todos_api:
    get("/todos"; json(.state.todos | [.[]]))

  , get("/todos/:id";
      .state.todos[.params.id] as $t
      | if $t == null then not_found else json($t) end)

  , post("/todos";
      . as $req
      | if ($req.json.title | type) != "string" then bad_request("expected {\"title\": \"...\"}")
        else ($req.state.next | tostring) as $id
        | {id: $id, title: $req.json.title, done: false} as $t
        | json($t) | status(201) | header("Location"; "/api/todos/\($id)")
        | set_state($req.state | .todos[$id] = $t | .next += 1)
        end)

  , patch("/todos/:id";
      . as $req
      | $req.state.todos[$req.params.id] as $t
      | if $t == null then not_found
        else ($t + (($req.json // {}) | {title, done} | with_entries(select(.value != null)))) as $updated
        | json($updated) | set_state($req.state | .todos[$req.params.id] = $updated)
        end)

  , delete("/todos/:id";
      . as $req
      | if $req.state.todos[$req.params.id] == null then not_found
        else {status: 204, headers: {}, body: ""} | set_state($req.state | del(.todos[$req.params.id]))
        end)
  ;

def app:
    get("/"; html("<h1>jqpress</h1><p>A web framework written in jq. Try <code>GET /api/todos</code>.</p>"))
  , get("/hello/:name"; text("hello, \(.params.name)!"))
  , get("/search"; json({you_searched_for: .query.q, page: (.query.page // "1" | tonumber)}))
  , get("/boom"; error("something went wrong"))
  , mount("/api"; cors(todos_api))
  ;

serve_state($ARGS.named.port // "8080"; {todos: {}, next: 1}; app)
