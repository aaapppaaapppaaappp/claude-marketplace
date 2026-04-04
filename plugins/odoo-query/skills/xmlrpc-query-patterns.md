---
name: xmlrpc-query-patterns
keywords: [xmlrpc, query, api, external, read, search, investigate]
description: Patterns for querying Odoo via XML-RPC API
---

# Odoo XML-RPC Query Patterns

Reference for performing read-only queries against Odoo instances.

## Connection Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│  Client (Claude)                                                │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  odoo_xmlrpc.py                                                 │
│       │                                                         │
│       ▼                                                         │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │ XML-RPC Connection                                       │   │
│  │   URL/xmlrpc/2/common  → Authentication                 │   │
│  │   URL/xmlrpc/2/object  → Model Operations               │   │
│  └─────────────────────────────────────────────────────────┘   │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  Odoo Server                                                    │
├─────────────────────────────────────────────────────────────────┤
│  /xmlrpc/2/common                                               │
│    - version()          → Server version info                   │
│    - authenticate()     → Get user ID                           │
│                                                                 │
│  /xmlrpc/2/object                                               │
│    - execute_kw()       → Run model methods                     │
│      └─ search          → Find record IDs                       │
│      └─ read            → Read record data                      │
│      └─ search_read     → Search + Read combined                │
│      └─ search_count    → Count matching records                │
│      └─ fields_get      → Get model field definitions           │
└─────────────────────────────────────────────────────────────────┘
```

## Domain Filter Reference

### Basic Operators
| Operator | Description | Example |
|----------|-------------|---------|
| `=` | Equals | `[('state', '=', 'draft')]` |
| `!=` | Not equals | `[('state', '!=', 'done')]` |
| `>` | Greater than | `[('amount', '>', 1000)]` |
| `>=` | Greater or equal | `[('date', '>=', '2024-01-01')]` |
| `<` | Less than | `[('qty', '<', 10)]` |
| `<=` | Less or equal | `[('priority', '<=', 2)]` |
| `in` | In list | `[('state', 'in', ['draft', 'sent'])]` |
| `not in` | Not in list | `[('state', 'not in', ['done', 'cancel'])]` |
| `like` | SQL LIKE (case-sensitive) | `[('name', 'like', 'SO%')]` |
| `ilike` | LIKE (case-insensitive) | `[('email', 'ilike', '%@gmail.com')]` |
| `=like` | Pattern match | `[('name', '=like', 'SO___')]` |
| `=ilike` | Pattern (case-insensitive) | `[('code', '=ilike', 'prod_%')]` |
| `child_of` | Hierarchical child | `[('category_id', 'child_of', 5)]` |
| `parent_of` | Hierarchical parent | `[('category_id', 'parent_of', 10)]` |

### Logical Operators
```python
# AND (implicit - default)
[('state', '=', 'sale'), ('amount', '>', 1000)]

# AND (explicit)
['&', ('state', '=', 'sale'), ('amount', '>', 1000)]

# OR
['|', ('state', '=', 'draft'), ('state', '=', 'sent')]

# NOT
['!', ('active', '=', True)]

# Complex: (A AND B) OR C
['|', '&', ('state', '=', 'sale'), ('amount', '>', 1000), ('priority', '=', 'high')]

# Complex: A AND (B OR C)
['&', ('state', '=', 'sale'), '|', ('amount', '>', 1000), ('priority', '=', 'high')]
```

### Date/Time Filters
```python
# Exact date
[('date_order', '=', '2024-01-15')]

# Date range
[('date_order', '>=', '2024-01-01'), ('date_order', '<=', '2024-12-31')]

# Today (use Python to compute)
# today = datetime.date.today().isoformat()
[('date_order', '=', '2024-01-20')]

# Last 30 days
# thirty_days_ago = (datetime.date.today() - datetime.timedelta(days=30)).isoformat()
[('create_date', '>=', '2023-12-21')]
```

### Relational Field Filters
```python
# Filter by related record ID
[('partner_id', '=', 42)]

# Filter by related field value (dot notation)
[('partner_id.country_id.code', '=', 'US')]

# Filter by Many2many (contains)
[('tag_ids', 'in', [1, 2, 3])]

# Empty relation
[('partner_id', '=', False)]

# Non-empty relation
[('partner_id', '!=', False)]
```

## Database Exploration Tools

### Describe Model - Comprehensive Model Information

The `describe_model` action consolidates multiple queries into one call, providing everything needed for migration planning or understanding a model's structure.

**Problem it solves:**
- `fields_get` returns `None` for selection values
- Need to query `ir.model.fields` separately to get selection options
- Multiple calls required to understand a model completely

**What it returns:**
- All fields with type, string, required, readonly
- Selection values from ir.model.fields (not fields_get)
- List of required fields for quick reference
- Relational fields with their target models
- Help text for field documentation

```bash
# Get comprehensive model description
python3 ${CLAUDE_PLUGIN_ROOT}/scripts/odoo_xmlrpc.py \
  --url "$URL" --db "$DB" --login "$LOGIN" --api-key "$API_KEY" \
  --action describe_model \
  --model "sale.order"
```

**Example output:**
```json
{
  "success": true,
  "action": "describe_model",
  "model": "sale.order",
  "field_count": 156,
  "required_fields": ["partner_id", "date_order"],
  "relational_fields": {
    "partner_id": "res.partner",
    "user_id": "res.users",
    "company_id": "res.company",
    "order_line": "sale.order.line"
  },
  "fields": {
    "state": {
      "type": "selection",
      "string": "Status",
      "required": true,
      "readonly": false,
      "selection": [
        ["draft", "Quotation"],
        ["sent", "Quotation Sent"],
        ["sale", "Sales Order"],
        ["done", "Locked"],
        ["cancel", "Cancelled"]
      ]
    },
    "partner_id": {
      "type": "many2one",
      "string": "Customer",
      "required": true,
      "readonly": false,
      "relation": "res.partner"
    }
  }
}
```

**Use cases:**
- Migration planning: identify required fields before data import
- Understanding custom models without documentation
- Checking selection values for state fields
- Finding relational dependencies

### Find Model - Fuzzy Search with Record Counts

The `find_model` action searches across model names and descriptions with record counts, helping identify the right model when multiple similar ones exist.

**Problem it solves:**
- Ambiguous model names (e.g., "gwr.pae.cycle" - is this catalog or student-facing?)
- Finding all models related to a concept (e.g., all "invoice" models)
- Identifying active vs. unused models
- Discovering custom models without browsing full list

**What it returns:**
- Model technical name
- Model display name
- Model description/info
- Record count (shows if model is actively used)

```bash
# Find all models related to cycles
python3 ${CLAUDE_PLUGIN_ROOT}/scripts/odoo_xmlrpc.py \
  --url "$URL" --db "$DB" --login "$LOGIN" --api-key "$API_KEY" \
  --action find_model \
  --keyword "cycle"
```

**Example output:**
```json
{
  "success": true,
  "action": "find_model",
  "keyword": "cycle",
  "count": 3,
  "models": [
    {
      "model": "gwr.pae.cycle",
      "name": "Student PAE Cycles",
      "info": "Manages student planning cycles",
      "record_count": 847
    },
    {
      "model": "gwr.catalog.cycle",
      "name": "Course Catalog Cycles",
      "info": "Academic year catalog definitions",
      "record_count": 12
    },
    {
      "model": "gwr.cycle.archive",
      "name": "Archived Cycles",
      "info": "Historical cycle data",
      "record_count": 0
    }
  ]
}
```

**Use cases:**
- Finding the right model when naming is ambiguous
- Discovering related models (search "invoice" finds account.move, account.payment, etc.)
- Identifying unused/test models (record_count = 0)
- Understanding model purposes through record counts

**Common searches:**
```bash
# Find all invoice-related models
--action find_model --keyword "invoice"

# Find payment models
--action find_model --keyword "payment"

# Find all models with "student" in name
--action find_model --keyword "student"

# Find custom models (search for company prefix)
--action find_model --keyword "gwr"
```

## Common Query Patterns

### Sales Investigation
```bash
# Recent confirmed sales
--action search_read --model "sale.order" \
  --domain "[('state', '=', 'sale'), ('date_order', '>=', '2024-01-01')]" \
  --fields "name,partner_id,date_order,amount_total,user_id" \
  --limit 20 --order "date_order desc"

# Large orders
--action search_read --model "sale.order" \
  --domain "[('amount_total', '>', 10000)]" \
  --fields "name,partner_id,amount_total,state"

# Orders by specific salesperson
--action search_read --model "sale.order" \
  --domain "[('user_id.name', 'ilike', 'john')]" \
  --fields "name,partner_id,amount_total,state"
```

### Invoice Investigation
```bash
# Overdue invoices
--action search_read --model "account.move" \
  --domain "[('move_type', '=', 'out_invoice'), ('payment_state', '!=', 'paid'), ('invoice_date_due', '<', '2024-01-20')]" \
  --fields "name,partner_id,amount_total,invoice_date_due,payment_state"

# Invoices for specific customer
--action search_read --model "account.move" \
  --domain "[('partner_id', '=', 42), ('move_type', '=', 'out_invoice')]" \
  --fields "name,amount_total,state,payment_state,invoice_date"
```

### Inventory Investigation
```bash
# Low stock products
--action search_read --model "stock.quant" \
  --domain "[('quantity', '<', 10), ('location_id.usage', '=', 'internal')]" \
  --fields "product_id,location_id,quantity,reserved_quantity"

# Product availability
--action search_read --model "product.product" \
  --domain "[('type', '=', 'product'), ('qty_available', '>', 0)]" \
  --fields "name,default_code,qty_available,virtual_available"
```

### Customer/Partner Investigation
```bash
# Customers with credit
--action search_read --model "res.partner" \
  --domain "[('customer_rank', '>', 0), ('credit', '>', 0)]" \
  --fields "name,email,credit,credit_limit"

# Partners by country
--action search_read --model "res.partner" \
  --domain "[('country_id.code', '=', 'FR')]" \
  --fields "name,city,email,phone"
```

### User/Access Investigation
```bash
# Users in specific group
--action search_read --model "res.users" \
  --domain "[('groups_id.name', 'ilike', 'sales')]" \
  --fields "name,login,groups_id"

# Inactive users
--action search_read --model "res.users" \
  --domain "[('active', '=', False)]" \
  --fields "name,login,write_date"
```

### Custom Model Investigation
```bash
# Step 1: Find the model if name is uncertain
--action find_model --keyword "partial_name"

# Step 2: Get comprehensive model description
--action describe_model --model "custom.model.name"

# Step 3: Check for sample records
--action search_read --model "custom.model.name" \
  --domain "[]" \
  --fields "name,state" \
  --limit 5

# Step 4: Query based on findings
--action search_read --model "custom.model.name" \
  --domain "[('state', '=', 'active')]" \
  --fields "name,field1,field2,create_date"
```

**Legacy approach (less efficient):**
```bash
# Old way: use fields_get (doesn't return selection values)
--action fields_get --model "custom.model.name"

# Then query ir.model.fields separately for selections
--action search_read --model "ir.model.fields" \
  --domain "[('model_id.model', '=', 'custom.model.name')]" \
  --fields "name,ttype,selection"
```

## Troubleshooting

### Authentication Errors
- Verify URL includes protocol (https://)
- Check database name is exact (case-sensitive)
- Ensure API key is valid (Settings > Users > API Keys)
- Confirm user is active

### Access Denied Errors
- User may not have read access to the model
- Check user's groups and access rights
- Try with admin user to verify

### Model Not Found
- Verify model technical name (e.g., `sale.order` not `Sales Order`)
- Use `list_models` action to see available models
- Module containing model may not be installed

### Field Not Found
- Use `fields_get` to see available fields
- Field may have different technical name
- Field may be computed and not stored

## Security Best Practices

1. **Use API Keys** - Prefer API keys over passwords
2. **Minimal Permissions** - Use a user with only necessary read access
3. **No Credentials in Code** - Pass credentials as arguments
4. **Audit Trail** - Queries are logged in Odoo's access logs
5. **Rate Limiting** - Don't overwhelm the server with rapid queries
